# Anchor Panic Ps

## How to use
- install rust
- run cargo build --release
- redirect game to localhost:8702 eg 192.x
- use Dawn [patch](https://github.com/yoncodes/dawn-patch)

## features 
- login works
- tcpserver can be run in proxy mode
- tcpserver --proxy "gamehost:gameport"
- Saves and logs packets in realtime as jsonl file
## Note not uploading data files for now

## Not working
- a lot 
- httpserver doesn't work need to redo it

## Future plans
- Data loader is used temporally to load packet format as json. Eventually I'd want to use a db instead.
- Code is a mess most of it needs to be rewritten. 
