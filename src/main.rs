use std::io::{IsTerminal, Read};
use std::str::FromStr;
use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{anyhow, Context, Error};
use chrono::{DateTime, Local};

use nostr_sdk::prelude::*;

// Basic protocol flow description
// https://github.com/nostr-protocol/nips/blob/master/01.md

// Extra metadata fields and tags
// https://github.com/nostr-protocol/nips/blob/master/24.md

// Lists
// https://github.com/nostr-protocol/nips/blob/master/51.md

// Relay List Metadata
// https://github.com/nostr-protocol/nips/blob/master/65.md

// Private Direct Messages
// https://github.com/nostr-protocol/nips/blob/master/17.md

type JsonOrdered = indexmap::IndexMap<String, serde_json::Value>;

enum KeyTypeFormat {
    SecretHex,
    SecretBech32,
    PublicHex,
    PublicBech32
}

impl FromStr for KeyTypeFormat {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "shex" => Self::SecretHex,
            "sbech32" => Self::SecretBech32,
            "phex" => Self::PublicHex,
            "pbech32" => Self::PublicBech32,
            _ => return Err(anyhow!("can not parse {s}"))
        })
    }
}

// utils

fn path_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

fn mkdir(path: &str) -> Result<(), Error> {
    std::fs::create_dir_all(std::path::PathBuf::from(&path))?;

    Ok(())
}

fn file_write(file_name: &str, content: &str) -> Result<(), Error> {
    Ok(std::fs::write(file_name, content)?)
}

fn json_to_string_pretty<T: ?Sized>(json_value: &T) -> String
where
    T: serde::ser::Serialize
{
    serde_json::to_string_pretty(&json_value)
        .unwrap_or_else(|_| format!("error: getting json string pretty"))
}

fn read_stdin_pipe() -> Result<String, Error> {
    let mut input = std::io::stdin();

    if input.is_terminal() {
        return Err(anyhow!("stdin is empty"));
    }

    let mut output = String::new();

    input.read_to_string(&mut output)?;

    Ok(output)
}

fn datetime_human_readable_format_get() -> &'static str {
    "%Y/%m/%d %H:%M:%S"
}

fn unix_timestamp_s_to_string(timestamp: u64) -> Result<String, Error> {
    let datetime =
        DateTime::from_timestamp(timestamp as i64, 0)
            .ok_or(anyhow!("datetime from timestamp seconds"))?
            .with_timezone(&Local);

    Ok(datetime.format(datetime_human_readable_format_get()).to_string())
}

fn u64_from_serde_value(
    object: &serde_json::Value, key: &str
) -> Result<u64, Error> {
    Ok(object.get(key)
        .ok_or(anyhow!("{key} not present"))?
        .as_number()
        .ok_or(anyhow!("{key} not number"))?
        .as_u64()
        .ok_or(anyhow!("{key} not u64"))?
    )
}

fn timeout_get() -> Duration {
    Duration::from_secs(60)
}

// actions

fn key_convert(key: &str, key_type_format: KeyTypeFormat) -> Result<(), Error> {
    let key_output = if let Ok(keys) = Keys::parse(key) {
        match key_type_format {
            KeyTypeFormat::SecretHex => keys.secret_key().to_secret_hex(),
            KeyTypeFormat::SecretBech32 => keys.secret_key().to_bech32()?,
            KeyTypeFormat::PublicHex => keys.public_key().to_hex(),
            KeyTypeFormat::PublicBech32 => keys.public_key().to_bech32()?
        }
    } else if let Ok(public_key) = PublicKey::parse(key) {
        match key_type_format {
            KeyTypeFormat::SecretHex|KeyTypeFormat::SecretBech32 =>
                return Err(anyhow!("can not get private key from public key")),
            KeyTypeFormat::PublicHex => public_key.to_hex(),
            KeyTypeFormat::PublicBech32 => public_key.to_bech32()?
        }
    } else {
        return Err(anyhow!("stdin is not a secret key nor a public key"));
    };

    println!("{key_output}");

    Ok(())
}

fn unsigned_event_print(
    event: UnsignedEvent,
    extra_fields: Option<JsonOrdered>
) -> Result<(), Error> {
    let mut event_json = JsonOrdered::new();

    event_json.insert(
        format!("id"),
        serde_json::json!(event.id)
    );

    event_json.insert(
        format!("pubkey"),
        serde_json::json!({
            "bech32": event.pubkey.to_bech32()?,
            "hex": event.pubkey.to_hex()
        })
    );

    event_json.insert(
        format!("created_at"),
        serde_json::json!({
            "timestamp": event.created_at.as_u64(),
            "date": serde_json::json!(
                unix_timestamp_s_to_string(event.created_at.as_u64())?
            )
        })
    );

    event_json.insert(
        format!("kind"),
        serde_json::json!(event.kind)
    );

    event_json.insert(
        format!("tags"),
        serde_json::json!(event.tags.as_slice())
    );

    event_json.insert(
        format!("content"),
        serde_json::from_str::<serde_json::Value>(&event.content)
            .unwrap_or_else(|_| serde_json::json!(&event.content))
    );

    if let Some(extra_fields) = extra_fields {
        event_json.extend(extra_fields);
    }

    println!("{}", json_to_string_pretty(&event_json));

    Ok(())
}

// create client with tor, connect it to the relays and return it
async fn client_connected_relays_get(
    relays_list: &Vec<Vec<String>>
) -> Result<Client, Error> {
    let timeout = timeout_get();

    let tor_socket: SocketAddr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9050
    );

    let client = Client::builder()
        .opts(ClientOptions::new()
            .gossip(false)
            .connection(Connection::new().proxy(tor_socket))
        ).build();

    for relays in relays_list {
        for relay in relays {
            client.add_relay(relay).await?;
        }
    }
    for (key, value) in client.try_connect(timeout).await.failed {
        eprintln!("error: {key} connecting {value}");
    }

    Ok(client)
}

async fn events_fetch_filter(
    filter: Filter, relays: Vec<String>
) -> Result<Events, Error> {
    let timeout = timeout_get();

    let client = client_connected_relays_get(&vec![relays]).await?;

    let events: Events = client
        .fetch_events(filter, timeout)
        .await?;

    client.disconnect().await;

    Ok(events)
}

async fn events_send(
    events: Vec<Event>, relays_list: Vec<Vec<String>>
) -> Result<(), Error> {
    if relays_list.len() != 1 && relays_list.len() != events.len() {
        return Err(anyhow!(
            "relays list should be len 1 or the same \
             as the len of the events"
        ));
    }

    let client = client_connected_relays_get(&relays_list).await?;

    for i in 0..events.len() {
        let relays = if relays_list.len() == 1 {
            &relays_list[0]
        } else {
            &relays_list[i]
        };

        for (key, value) in client.send_event_to(
            relays, &events[i]
        ).await?.failed {
            eprintln!("error: {key} sending event {value}");
        }

        println!("event {} sent", i + 1);
    }

    client.disconnect().await;

    Ok(())
}

fn metadata_event(
    metadata: Metadata, private_key: &str
) -> Result<(), Error> {
    let keys = Keys::parse(private_key)?;
    let event = EventBuilder::metadata(&metadata).sign_with_keys(&keys)?;

    println!("{}", event.as_pretty_json());

    Ok(())
}

// currently specifying just read/write for nip-65 not supported
fn relay_list_event(
    kind: Kind, private_key: &str, relays: Vec<String>
) -> Result<(), Error> {
    let keys = Keys::parse(private_key)?;

    let tag_kind = TagKind::Custom(std::borrow::Cow::Borrowed(match &kind {
        Kind::RelayList => "r",
        Kind::InboxRelays => "relay",
        _ => return Err(anyhow!("wrong kind"))
    }));

    let mut builder = EventBuilder::new(kind, "");
    for relay in relays {
        builder = builder.tag(Tag::custom(tag_kind.clone(), [relay]));
    }

    let event = builder.sign_with_keys(&keys)?;

    println!("{}", event.as_pretty_json());

    Ok(())
}

fn filter_add_options(
    mut filter: Filter, since: Option<u64>, until: Option<u64>
) -> Filter {
    if let Some(since) = since {
        filter = filter.since(Timestamp::from(since))
    }
    if let Some(until) = until {
        filter = filter.until(Timestamp::from(until))
    }

    filter
}

async fn events_fetch(
    kinds: Vec<Kind>, public_key: &str, relays: Vec<String>,
    since: Option<u64>, until: Option<u64>
) -> Result<(), Error> {
    let mut filter: Filter = Filter::new()
        .authors([PublicKey::parse(public_key)?])
        .kinds(kinds);
    filter = filter_add_options(filter, since, until);

    let events = events_fetch_filter(filter, relays).await?;

    for event in events.to_vec() {
        println!("{}", event.as_pretty_json());
    }

    Ok(())
}

fn rumors_info<T>(
    rumors: Vec<T>
) -> Result<(), Error>
where
    T: Into<UnsignedEvent>
{
    for rumor in rumors {
        unsigned_event_print(rumor.into(), None)?;
    }

    Ok(())
}

// https://github.com/nostr-protocol/nips/blob/master/17.md
//
// TODO: support tags:
// ["e", "<kind-14-id>", "<relay-url>"] // if this is a reply
// ["subject", "<conversation-title>"]
// ["q", "<event-id> or <event-address>", "<relay-url>", "<pubkey-if-a-regular-event>"]
//
// TODO: support Kind::Custom(15)
async fn dm_events(
    private_key: &str, receiver_public_key: &str, message: &str
) -> Result<(), Error> {
    let keys = Keys::parse(private_key)?;
    let receiver = PublicKey::parse(receiver_public_key)?;

    let rumor: UnsignedEvent = EventBuilder::new(Kind::Custom(14), message)
        .tags([Tag::public_key(receiver)])
        .build(keys.public_key());

    let event_receiver: Event = EventBuilder::gift_wrap(
        &keys, &receiver, rumor.clone(), []
    ).await?;

    let event_self: Event = EventBuilder::gift_wrap(
        &keys, &keys.public_key(), rumor, []
    ).await?;

    println!("{}", event_receiver.as_pretty_json());
    println!("{}", event_self.as_pretty_json());

    Ok(())
}

// should be renamed gift_wraps_fetch then maybe an other function specific for
// nip-17 private direct messages
async fn dm_fetch(
    private_key: &str, relays: Vec<String>
) -> Result<(), Error> {
    let keys = Keys::parse(private_key)?;

    let filter: Filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkey(keys.public_key());

    let events = events_fetch_filter(filter, relays).await?;

    for event in events.to_vec() {
        if event.kind == Kind::GiftWrap {
            let UnwrappedGift { sender, rumor } =
                UnwrappedGift::from_gift_wrap(&keys, &event).await?;

            let mut extra_fields =
                JsonOrdered::new();

            if sender != rumor.pubkey {
                extra_fields.insert(
                    format!("warning"),
                    serde_json::json!(format!(
                        "pubkey of the sealed event is not the same as the \
                         one in the rumor"
                    ))
                );
                extra_fields.insert(
                    format!("sealed"),
                    serde_json::json!({
                        "bech32": sender.to_bech32()?,
                        "hex": sender.to_hex()
                    })
                );
            }

            // TODO: handle Kind::Custom(15) better
            unsigned_event_print(rumor, Some(extra_fields))?;
        }
    }

    Ok(())
}

fn dm_save(
    messages: Vec<JsonOrdered>, public_key: &str, dir_save: &str
) -> Result<(), Error> {
    let self_public_key = PublicKey::parse(public_key)?;

    if ! path_exists(dir_save) {
        mkdir(dir_save)?;
    }

    for message in messages {
        let sender_bech32 = message.get("pubkey")
            .ok_or(anyhow!("sender not present"))?
            .as_object()
            .ok_or(anyhow!("sender not obj"))?
            .get("bech32")
            .ok_or(anyhow!("bech32 not present"))?
            .as_str()
            .ok_or(anyhow!("bech32 not str"))?;
        let peer_bech32 = if sender_bech32 != self_public_key.to_bech32()? {
            sender_bech32
        } else {
            // TODO: handle multiple p tags

            let mut tag_public_key: Option<&str> = None;

            for tag in message.get("tags")
                .ok_or(anyhow!("tags not present"))?
                .as_array()
                .ok_or(anyhow!("tags not array"))? {
                let tag_array = tag
                    .as_array()
                    .ok_or(anyhow!("tag not array"))?;
                if tag_array[0].as_str()
                    .ok_or(anyhow!("tag element 0 not str"))?
                == "p" && tag_array.len() >= 2 {
                    tag_public_key = Some(tag_array[1]
                        .as_str()
                        .ok_or(anyhow!("tag element 1 not str"))?
                    );
                    break;
                }
            }

            match tag_public_key {
                Some(public_key) => {
                    &PublicKey::parse(public_key)?.to_bech32()?
                },
                None => return Err(anyhow!("p tag not found"))
            }
        };

        let peer_dir = format!("{dir_save}/{peer_bech32}");
        if ! path_exists(&peer_dir) {
            mkdir(&peer_dir)?;
        }

        let message_id = message.get("id")
            .ok_or(anyhow!("id not present"))?
            .as_str()
            .ok_or(anyhow!("id not str"))?;

        let created_at = message.get("created_at")
            .ok_or(anyhow!("created_at not present"))?
            .as_object()
            .ok_or(anyhow!("created_at not object"))?
            .get("timestamp")
            .ok_or(anyhow!("timestamp not present"))?
            .as_number()
            .ok_or(anyhow!("timestamp not number"))?
            .as_u64()
            .ok_or(anyhow!("timestamp not u64"))?;

        let message_file = format!("{peer_dir}/{created_at}-{message_id}");
        if ! path_exists(&message_file) {
            file_write(
                &message_file, &(json_to_string_pretty(&message) + "\n")
            )?;
        }
    }

    Ok(())
}

fn stdin_events_array<T>() -> Result<Vec<T>, Error>
where
    T: Into<UnsignedEvent> + for<'a>serde::Deserialize<'a>
{
    let mut events: Vec<T> = Vec::new();

    for event in serde_json::Deserializer::from_str(
        &read_stdin_pipe()
            .with_context(|| "reading events in stdin")?
    ).into_iter() {
        events.push(event
            .with_context(|| "deserializing event")?
        );
    }

    Ok(events)
}

fn stdin_key() -> Result<String, Error> {
    Ok(read_stdin_pipe()
        .with_context(|| "reading key in stdin")?
        .trim()
        .to_owned()
    )
}

fn arg_relay_array(current_parameter: usize) -> Result<Vec<String>, Error> {
    let relays: Vec<String> = serde_json::from_str(
        &std::env::args().nth(current_parameter)
            .ok_or(anyhow!("insert relays array"))?
    ).with_context(|| "parsing relays array")?;

    Ok(relays)
}

fn arg_filter_options(
    current_parameter: usize
) -> Result<(Option<u64>, Option<u64>), Error> {
    let mut since: Option<u64> = None;
    let mut until: Option<u64> = None;

    let options: serde_json::Value = serde_json::from_str(
        &std::env::args().nth(current_parameter)
            .ok_or(anyhow!("insert filter options"))?
    ).with_context(|| "parsing filter options")?;

    if let Ok(value) = u64_from_serde_value(&options, "since") {
        since = Some(value);
    }
    if let Ok(value) = u64_from_serde_value(&options, "until") {
        until = Some(value);
    }

    Ok((since, until))
}

async fn handle_arguments() -> Result<(), Error> {
    let mut current_parameter: usize = 1;
    match std::env::args().nth(current_parameter).as_deref() {
        Some(arg) => match arg {
            "-h" | "--help" => {
                print!(
r#"nmini action

actions:
<key> | key-convert shex|sbech32|phex|pbech32
<events> | events-send <relays>...
<private-key> | metadata-event <metadata-json>
<private-key> | relay-list-event [standard|inbox] <relays>
events-fetch <public-key> <kinds> <relays> <filter-options>
<rumors> | rumors-info
<private-key> | dm-events <public-key> <message>
<private-key> | dm-fetch <relays>
<messages> | dm-save <public-key> <dir>

args:
private-key and public-key can be hex or bech32
key can be private-key or public-key
events is a list of signed json nostr events
rumors is a list of signed or unsiged json nostr events
relays is a json array of string urls
metadata-json is a json object that is parsed as metadata (nip-01, nip-24)
kinds is a json array of kinds (uint)
filter-options is a json object that can have fields since and until
messages is a list of json object messages
"#
                );
            },
            "key-convert" => {
                current_parameter += 1;
                let key_type_format = KeyTypeFormat::from_str(
                    &std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert key type format"))?
                ).with_context(|| "parsing key type format")?;

                let key = stdin_key()?;

                key_convert(&key, key_type_format)?;
            },
            "events-send" => {
                let mut relays_list: Vec<Vec<String>> = Vec::new();
                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    relays_list.push(arg_relay_array(current_parameter)?);
                }

                let events: Vec<Event> = stdin_events_array()?;

                events_send(events, relays_list).await?;
            },
            "metadata-event" => {
                current_parameter += 1;
                let metadata = Metadata::from_json(
                    &std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert metadata json"))?
                ).with_context(|| "parsing metadata json")?;

                let private_key = stdin_key()?;

                metadata_event(metadata, &private_key)?;
            },
            "relay-list-event" => {
                current_parameter += 1;
                let relay_type = match std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert relay type"))?.as_str() {
                    "standard" => Kind::RelayList,
                    "inbox" => Kind::InboxRelays,
                    relay_type_arg => return Err(anyhow!(
                        "{relay_type_arg} is not a relay type for {arg}"
                    ))
                };

                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                let private_key = stdin_key()?;

                relay_list_event(relay_type, &private_key, relays)?;
            },
            "events-fetch" => {
                current_parameter += 1;
                let public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert public key"))?;

                current_parameter += 1;
                let relay_types: Vec<Kind> = serde_json::from_str(
                    &std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert relay types"))?
                ).with_context(|| "parsing relay types")?;

                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                current_parameter += 1;
                let (since, until) = arg_filter_options(current_parameter)?;

                events_fetch(
                    relay_types, &public_key, relays, since, until
                ).await?;
            },
            "rumors-info" => {
                let rumors: Vec<UnsignedEvent> = stdin_events_array()?;

                rumors_info(rumors)?;
            }
            "dm-events" => {
                current_parameter += 1;
                let receiver_public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert receiver public key"))?;

                current_parameter += 1;
                let message = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert message"))?;

                let private_key = stdin_key()?;

                dm_events(&private_key, &receiver_public_key, &message).await?;
            },
            "dm-fetch" => {
                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                // no since gift wrapped events have randomized created_at
                // current_parameter += 1;
                // let (since, until) = arg_filter_options(current_parameter)?;

                let private_key = stdin_key()?;

                dm_fetch(&private_key, relays).await?;
            },
            "dm-save" => {
                current_parameter += 1;
                let public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert public key"))?;

                current_parameter += 1;
                let dir_save = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert dir"))?;

                let mut messages: Vec<JsonOrdered> = Vec::new();
                for message in serde_json::Deserializer::from_str(
                    &read_stdin_pipe()
                        .with_context(|| "reading messages in stdin")?
                ).into_iter() {
                    messages.push(message
                        .with_context(|| "deserializing message")?
                    );
                }

                dm_save(messages, &public_key, &dir_save)?;
            },
            _ => return Err(anyhow!("argument {arg} not recognized"))
        },
        None => return Err(anyhow!("insert action"))
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    match handle_arguments().await {
        Ok(_) => {
            std::process::exit(0);
        },
        Err(error_chain) => {
            eprintln!("error");
            for error in error_chain.chain().rev() {
                eprintln!("{}", error.to_string());
            }
            std::process::exit(1);
        }
    }
}
