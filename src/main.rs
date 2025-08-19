mod utils;

use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{anyhow, Context, Error};
use url::Url;

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

struct MetadataVar {
    name: Option<String>,
    display_name: Option<String>,
    about: Option<String>,
    website: Option<String>,
    picture: Option<String>,
    banner: Option<String>
}

impl Default for MetadataVar {
    fn default() -> Self {
        Self {
            name: None,
            display_name: None,
            about: None,
            website: None,
            picture: None,
            banner: None
        }
    }
}

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

async fn events_fetch(
    filter: Filter, relays: Vec<String>
) -> Result<Events, Error> {
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
        println!("{key} connecting {value}");
    }

    let events: Events = client
        .fetch_events(filter, timeout)
        .await?;

    client.disconnect().await;

    Ok(events)
}

async fn event_send(
    event: Event, relays: Vec<String>
) -> Result<(), Error> {
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

    for (key, value) in client.send_event(&event).await?.failed {
        eprintln!("error: {key} sending event {value}");
    }

    println!("event sent");

    client.disconnect().await;

    Ok(())
}

fn metadata_event(
    mtv: MetadataVar, private_key: &str
) -> Result<(), Error> {
    let mut metadata = Metadata::new();
    if let Some(name) = mtv.name {
        metadata = metadata.name(name);
    }
    if let Some(display_name) = mtv.display_name {
        metadata = metadata.display_name(display_name);
    }
    if let Some(about) = mtv.about {
        metadata = metadata.about(about);
    }
    if let Some(website) = mtv.website {
        metadata = metadata.website(Url::parse(&website)?);
    }
    if let Some(picture) = mtv.picture {
        metadata = metadata.picture(Url::parse(&picture)?);
    }
    if let Some(banner) = mtv.banner {
        metadata = metadata.banner(Url::parse(&banner)?);
    }

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

async fn handle_arguments() -> Result<(), Error> {
    let mut current_parameter = 1;
    match std::env::args().nth(current_parameter).as_deref() {
        Some(arg) => match arg {
            "-h" | "--help" => {
                print!(
r#"nmini action

<event> | event-send <relays>
<nsec> | metadata-event --[name|display-name|about|website|picture|banner]=<value> <relays>
metadata-fetch <public-key> <relays>
<nsec> | relay-list-event [standard|inbox] <relays>
relay-list-fetch [all|standard|inbox] <public-key> <relays>
"#
                );
            },
            "event-send" => {
                let mut relays = Vec::new();
                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    let relay = std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert relay"))?;
                    relays.push(relay);
                }

                let event = Event::from_json(
                    utils::read_stdin_pipe()
                        .with_context(|| "reading private key in stdin")?
                ).with_context(|| "parsing event")?;

                event_send(event, relays).await?;
            },
            "metadata-event" => {
                let mut mtv = MetadataVar::default();

                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    let current_arg = std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert argument"))?;

                    if ! current_arg.starts_with("--") {
                        // uncomment in case other arguments should be read
                        // current_parameter -= 1;
                        break
                    }

                    let full_option: Vec<&str> = current_arg.split('=').collect();
                    if full_option.len() < 2 {
                        return Err(anyhow!("argument {current_arg} not valid"));
                    }
                    let option_first_part = full_option[0];
                    let option_second_part = full_option[1];
                    let variable_to_set = match option_first_part {
                        "--name" => &mut mtv.name,
                        "--display_name" => &mut mtv.display_name,
                        "--about" => &mut mtv.about,
                        "--website" => &mut mtv.website,
                        "--picture" => &mut mtv.picture,
                        "--banner" => &mut mtv.banner,
                        _ => return Err(anyhow!(
                            "argument {current_arg} {option_first_part} not valid"
                        ))
                    };
                    if (*variable_to_set).is_some() {
                        return Err(anyhow!(
                            "{option_first_part} set multiple times")
                        );
                    }
                    *variable_to_set = Some(option_second_part.to_owned());
                }

                let private_key = utils::read_stdin_pipe()
                    .with_context(|| "reading private key in stdin")?
                    .trim()
                    .to_owned();

                metadata_event(mtv, &private_key)?;
            },
            "metadata-fetch" => {
                current_parameter += 1;
                let public_key = std::env::args().nth(current_parameter)
                    .ok_or(anyhow!("insert public key"))?;

                let mut relays = Vec::new();
                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    let relay = std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert relay"))?;
                    relays.push(relay);
                }

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

                let mut relays = Vec::new();
                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    let relay = std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert relay"))?;
                    relays.push(relay);
                }

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

                let mut relays = Vec::new();
                while std::env::args().len() - current_parameter > 1 {
                    current_parameter += 1;
                    let relay = std::env::args().nth(current_parameter)
                        .ok_or(anyhow!("insert relay"))?;
                    relays.push(relay);
                }

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
