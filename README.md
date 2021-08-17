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

## reclass
to run `betrayal_engine` in `reclass` mode type
```bash
cargo build --release && EDITOR=emacs sudo -HE ./target/release/betrayal_engine --pid=<PID> reclass
```
You can substitute emacs for other editor like VSCode. Won't work for vim unless you write a small custom script that opens a new terminal and then vim inside. Contributions are welcome.
