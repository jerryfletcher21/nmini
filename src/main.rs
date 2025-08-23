mod utils;

use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{anyhow, Context, Error};

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

fn timeout_get() -> Duration {
    Duration::from_secs(60)
}

fn unsigned_event_print(
    event: UnsignedEvent,
    extra_fields: Option<indexmap::IndexMap<String, serde_json::Value>>
) -> Result<(), Error> {
    let mut event_json = indexmap::IndexMap::<String, serde_json::Value>::new();

    event_json.insert(
        format!("kind"),
        serde_json::json!(event.kind)
    );

    event_json.insert(
        format!("created_at"),
        serde_json::json!(utils::unix_timestamp_s_to_string(
            event.created_at.as_u64())?
        )
    );

    event_json.insert(
        format!("content"),
        serde_json::from_str::<serde_json::Value>(&event.content)
            .unwrap_or_else(|_| serde_json::json!(&event.content))
    );

    event_json.insert(
        format!("tags"),
        serde_json::json!(event.tags.as_slice())
    );

    if let Some(extra_fields) = extra_fields {
        event_json.extend(extra_fields);
    }

//    let mut tags: Vec<Vec<String>> = Vec::new();
//    for tag in event.tags.as_slice() {
//        tags.push((&tag.as_slice()[1..]).to_vec());
//    }
//    event_json.insert(
//        format!("relays"),
//        serde_json::json!(tags)
//    );

    println!("{}", utils::json_to_string_pretty(&event_json));

    Ok(())
}

fn event_print(
    event: Event
) -> Result<(), Error> {
    unsigned_event_print(event.into(), None)
}

// create client with tor, connect it to the relays and return it
async fn client_connected_relays_get(
    relays: Vec<String>
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

    for relay in relays {
        client.add_relay(relay).await?;
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

    let client = client_connected_relays_get(relays).await?;

    let events: Events = client
        .fetch_events(filter, timeout)
        .await?;

    client.disconnect().await;

    Ok(events)
}

async fn event_send(
    event: Event, relays: Vec<String>
) -> Result<(), Error> {
    let client = client_connected_relays_get(relays).await?;

    for (key, value) in client.send_event(&event).await?.failed {
        eprintln!("error: {key} sending event {value}");
    }

    println!("event sent");

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

async fn events_fetch(
    kinds: Vec<Kind>, public_key: &str, relays: Vec<String>
) -> Result<(), Error> {
    let filter: Filter = Filter::new()
        .authors([PublicKey::parse(public_key)?])
        .kinds(kinds);

    let events = events_fetch_filter(filter, relays).await?;

    for event in events.to_vec() {
        event_print(event)?;
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
async fn dm_event(
    private_key: &str, receiver_public_key: &str, message: &str
) -> Result<(), Error> {
    let keys = Keys::parse(private_key)?;
    let receiver = PublicKey::parse(receiver_public_key)?;

    let rumor: UnsignedEvent = EventBuilder::new(Kind::Custom(14), message)
        .tags([Tag::public_key(receiver)])
        .build(keys.public_key());

    let event: Event = EventBuilder::gift_wrap(
        &keys, &receiver, rumor, []
    ).await?;

    println!("{}", event.as_pretty_json());

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
                indexmap::IndexMap::<String, serde_json::Value>::new();
            extra_fields.insert(
                format!("sender"),
                serde_json::json!({
                    "bech32": sender.to_bech32()?,
                    "hex": sender.to_hex()
                })
            );

            // TODO: handle Kind::Custom(15) better
            unsigned_event_print(rumor, Some(extra_fields))?;
        }
    }

    Ok(())
}

fn arg_relay_array(current_parameter: usize) -> Result<Vec<String>, Error> {
    let relays: Vec<String> = serde_json::from_str(
        &std::env::args().nth(current_parameter)
            .ok_or(anyhow!("insert relays array"))?
    ).with_context(|| "parsing relays array")?;

    Ok(relays)
}

async fn handle_arguments() -> Result<(), Error> {
    let mut current_parameter: usize = 1;
    match std::env::args().nth(current_parameter).as_deref() {
        Some(arg) => match arg {
            "-h" | "--help" => {
                print!(
r#"nmini action

actions:
<event> | event-send <relays>
<private-key> | metadata-event <metadata-json>
<private-key> | relay-list-event [standard|inbox] <relays>
events-fetch <public-key> <relay-types> <relays>
<private-key> | dm-event <public-key> <message>
<private-key> | dm-fetch <relays>

args:
private-key and public-key can be hex or bech32
event is a signed json nostr event
relays is a json array of string urls
metadata-json is a json object that is parsed as metadata (nip-01, nip-24)
relay-types is a json array of kinds (uint)
"#
                );
            },
            "event-send" => {
                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                let event = Event::from_json(
                    utils::read_stdin_pipe()
                        .with_context(|| "reading private key in stdin")?
                ).with_context(|| "parsing event")?;

                event_send(event, relays).await?;
            },
            "metadata-event" => {
                current_parameter += 1;
                let metadata = Metadata::from_json(
                    &std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert metadata json"))?
                ).with_context(|| "parsing metadata json")?;

                let private_key = utils::read_stdin_pipe()
                    .with_context(|| "reading private key in stdin")?
                    .trim()
                    .to_owned();

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

                let private_key = utils::read_stdin_pipe()
                    .with_context(|| "reading private key in stdin")?
                    .trim()
                    .to_owned();

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

                events_fetch(relay_types, &public_key, relays).await?;
            },
            "dm-event" => {
                current_parameter += 1;
                let receiver_public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert receiver public key"))?;

                current_parameter += 1;
                let message = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert message"))?;

                let private_key = utils::read_stdin_pipe()
                    .with_context(|| "reading private key in stdin")?
                    .trim()
                    .to_owned();

                dm_event(&private_key, &receiver_public_key, &message).await?;
            },
            "dm-fetch" => {
                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                let private_key = utils::read_stdin_pipe()
                    .with_context(|| "reading private key in stdin")?
                    .trim()
                    .to_owned();

                dm_fetch(&private_key, relays).await?;
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
