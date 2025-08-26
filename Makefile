PREFIX ?= $(HOME)/.local

build:
	cargo build --release --locked

install:
	cargo install --locked --path .
	mkdir -p $(PREFIX)/bin
	install -m 744 script/nminis $(PREFIX)/bin/
