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

fn tor_socket_get() -> SocketAddr {
    // let tor_host = "127.0.0.1";
    // let tor_port = 9050;
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9050
    )
}

fn event_print(
    event: &Event
) -> Result<(), Error> {
    let mut metadata_json = indexmap::IndexMap::new();

    metadata_json.insert(
        format!("kind"),
        serde_json::json!(event.kind)
    );

    metadata_json.insert(
        format!("created_at"),
        serde_json::json!(utils::unix_timestamp_s_to_string(
            event.created_at.as_u64())?
        )
    );

    metadata_json.insert(
        format!("content"),
        serde_json::from_str::<serde_json::Value>(&event.content)
            .unwrap_or_else(|_| serde_json::json!(&event.content))
    );

    metadata_json.insert(
        format!("tags"),
        serde_json::json!(event.tags.as_slice())
    );

//    let mut tags: Vec<Vec<String>> = Vec::new();
//    for tag in event.tags.as_slice() {
//        tags.push((&tag.as_slice()[1..]).to_vec());
//    }
//    metadata_json.insert(
//        format!("relays"),
//        serde_json::json!(tags)
//    );

    println!("{}", utils::json_to_string_pretty(&metadata_json));

    Ok(())
}

// create client with tor, connect it to the relays and return it
async fn client_connected_relays_get(
    relays: Vec<String>
) -> Result<Client, Error> {
    let timeout = timeout_get();

    let client = Client::builder()
        .opts(ClientOptions::new()
            .gossip(false)
            .connection(Connection::new().proxy(tor_socket_get()))
        ).build();

    for relay in relays {
        client.add_relay(relay).await?;
    }
    for (key, value) in client.try_connect(timeout).await.failed {
        eprintln!("error: {key} connecting {value}");
    }

    Ok(client)
}

async fn events_fetch(
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

async fn metadata_fetch(
    public_key: &str, relays: Vec<String>
) -> Result<(), Error> {
    let metadata_kind = Kind::Metadata;

    let filter: Filter = Filter::default()
        .authors([PublicKey::parse(public_key)?])
        .kinds([metadata_kind]);

    let events = events_fetch(filter, relays).await?;

    for event in events.to_vec() {
        event_print(&event)?;
    }

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

async fn relay_list_fetch(
    kinds: Vec<Kind>, public_key: &str, relays: Vec<String>
) -> Result<(), Error> {
    let filter: Filter = Filter::default()
        .authors([PublicKey::parse(public_key)?])
        .kinds(kinds);

    let events = events_fetch(filter, relays).await?;

    for event in events.to_vec() {
        event_print(&event)?;
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
metadata-fetch <public-key> <relays>
<private-key> | relay-list-event [standard|inbox] <relays>
relay-list-fetch [all|standard|inbox] <public-key> <relays>

args:
metadata-json and relays are parsed as json
metadata-json is an object that is parsed as metadata (nip-01, nip-24)
relays is an array of string urls
private-key and public-key can be hex or bech32
event is a signed json nostr event
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
            "metadata-fetch" => {
                current_parameter += 1;
                let public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert public key"))?;

                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                metadata_fetch(&public_key, relays).await?;
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
            "relay-list-fetch" => {
                current_parameter += 1;
                let relay_type = match std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert relay type"))?.as_str() {
                    "all" => vec!(Kind::RelayList, Kind::InboxRelays),
                    "standard" => vec!(Kind::RelayList),
                    "inbox" => vec!(Kind::InboxRelays),
                    relay_type_arg => return Err(anyhow!(
                        "{relay_type_arg} is not a relay type for {arg}"
                    ))
                };

                current_parameter += 1;
                let public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert public key"))?;

                current_parameter += 1;
                let relays = arg_relay_array(current_parameter)?;

                relay_list_fetch(relay_type, &public_key, relays).await?;
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
