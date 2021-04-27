# betrayal_engine
Cheat engine clone attempt

## running
```bash
# first terminal
cargo run --example test-program  # runs a test program
```

```bash
#second terminal
ps -aux | rg test-program  # note the PID
cargo build && sudo ./target/debug/betrayal_engine  # type the PID in a prompt
```
