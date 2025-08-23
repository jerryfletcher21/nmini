# nmini

### examples
```
# send message
$ pass nostr/nsec | nmini dm-events <receiver-public-key> "hello" | nmini events-send <receiver-inbox-relays> <self-inbox-relays>

# when sending to a new receiver for the first time
# send self inbox relays to receiver inbox relays
$ nmini events-fetch --raw <self-public-key> '[10050]' <self-relays> '{}' | nmini events-send "$(nmini events-fetch --raw <receiver-public-key> '[10050]' <receiver-relays> '{}' | jq '[ .tags[][1] ]')" "{}"

# fetch messages
pass nostr/nsec | nmini dm-fetch <self-inbox-relays> | nmini dm-save <self-public-key> <dir>

# list chatting peers
find <dir> -mindepth 1 -maxdepth 1 -exec basename '{}' \;

# print chat history
find <dir>/<chatting-peer> -mindepth 1 -maxdepth 1 -type f | sort -n | xargs -I '{}' cat '{}' | jq -r '. | "[\(.created_at.date)] \(.sender.bech32[4:8]) | \(.content)"'
```
