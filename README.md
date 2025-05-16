# Putput

Putput pipes your input to your specified commands and shows their outputs. It is meant to be always on, toggled with a keybinding, and used best with a keyboard.

<p align="center"> <img alt="pidif screenshot" width="400" src="https://github.com/user-attachments/assets/62bdaa88-cd6b-4d9c-802b-e13f00bc3c5e" /></p>

## Usage

Submit your input with <kbd>Enter</kbd>, and copy a specific result using its number, e.g. <kbd>Ctrl</kbd>+<kbd>1</kbd>.

## Configuration

Putput configuration will automatically be created at `~/.config/putput/config.toml`. It allows you to customize the app name, the commands array, and and whether to run the commands on every change automatically or not.

```toml
run_commands_on_change = false
title = "Taylor"
commands = [
  "trans --indent 0 --brief :sv",
  "trans --indent 0 --brief :en",
  "trans --indent 0 --brief :he --no-bidi",
  "wc",
]
```

## Installation

### Compiling manually

- `git clone` the repository
- Run `cargo build --release`
