build:
	cargo build --release --locked

install:
	cargo install --locked --path .
	install -m 744 script/nminis ~/.local/bin
