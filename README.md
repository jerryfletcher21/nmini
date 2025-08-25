# nmini

Really simple nostr cli tool for sending
[NIP-17](https://github.com/nostr-protocol/nips/blob/master/17.md) dms.

Right now there is just partial support for kind:15 encrypted file messages.

[nminis](script/nminis) is an example script using nmini.

Using [rust-nostr](https://github.com/rust-nostr/nostr).

```
$ nmini -h
nmini action

actions:
<key> | key-convert shex|sbech32|phex|pbech32
<events> | events-send <relays>...
<private-key> | metadata-event <metadata-json>
<private-key> | relay-list-event [standard|inbox] <relays>
events-fetch <public-key> <relay-types> <relays> <filter-options>
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
relay-types is a json array of kinds (uint)
filter-options is a json object that can have fields since and until
messages is a list of json object messages
```

### examples

```
# set nsec and npub as shell variables
NSEC="$(pass nostr/nsec)"
NPUB="$(echo "$NSEC" | nmini key-convert phex)"

# set standard (NIP-65) relays as shell variable
RELAYS='["wss://relay.damus.io", "wss://nos.lol", "wss://nostr.mom"]'

# publish relay list (NIP-65)
echo "$NSEC" | nmini relay-list-event standard "$RELAYS" | nmini events-send "$RELAYS"

# publish user metadata (kind:0)
echo "$NSEC" | nmini metadata-event '{"name": "nice-name", "about": "interesting-about", "website": "https://wonderful.website"}' | nmini events-send "$RELAYS"

# fetch relay list (NIP-65)
nmini events-fetch "$NPUB" '[10002]' "$RELAYS" "{}"

# fetch user metadata (kind:0) printing it nicely
nmini events-fetch "$NPUB" '[0]' "$RELAYS" "{}" | nmini rumors-info

# set inbox (NIP-17) relays as shell variable
INB_REL_SELF='["wss://relay.damus.io", "wss://nostr.bitcoiner.social", "wss://relay.primal.net"]'

# publish inbox relay list (NIP-17)
echo "$NSEC" | nmini relay-list-event inbox "$INB_REL_SELF" | nmini events-send "$INB_REL_SELF"

# publish inbox relay list (NIP-17) also to standard (NIP-65) relays
nmini events-fetch "$NPUB" '[10050]' "$INB_REL_SELF" "{}" | nmini events-send "$RELAYS"

# set peer npub as shell variable
NPUB_PEER="npub..."

# fetch peer standard (NIP-65) relays
RELAYS_PEER="$(nmini events-fetch "$NPUB_PEER" '[10002]' "$RELAYS" '{}' | jq -r '[ .tags[] | select(.[0] == "relay") | .[1] ]')"
# check if RELAYS_PEER is correct
echo "$RELAYS_PEER"

# fetch peer inbox (NIP-17) relays
INB_REL_PEER="$(nmini events-fetch "$NPUB_PEER" '[10050]' "$RELAYS_PEER" '{}' | jq -r '[ .tags[] | select(.[0] == "relay") | .[1] ]')"
# check if INB_REL_PEER is correct
echo "$INB_REL_PEER"

# send private direct message (NIP-17)
# NIP-17 messages require creating two events, both containing the same rumor
# (unsigned event), the first one gift wrapped to the peer, and the second one
# gift wrapped to ourself, so when fetching messages we can fetch also our
# sent messages
echo "$NSEC" | nmini dm-events "$NPUB_PEER" "hello" | nmini events-send "$INB_REL_PEER" "$INB_REL_SELF"

# when sending to a new receiver for the first time send our relays to the
# receiver relays
nmini events-fetch "$NPUB" '[10002]' "$RELAYS" '{}' | nmini events-send "$RELAYS_PEER" "{}"

# fetch messages
echo "$NSEC" | nmini dm-fetch "$INB_REL_SELF"

## nminis script

# fetch messages and save them to a directory
echo "$NSEC" | nminis dm-fetch-save "$INB_REL_SELF" ~/.local/share/nmini/"$NPUB"

# list chatting peers
find <dir> -mindepth 1 -maxdepth 1 -exec basename '{}' \;

# fzf chatting peers and print chat history
nminis chat-find-show ~/.local/share/nmini/"$NPUB"
```
