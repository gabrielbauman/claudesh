# claudesh

> **Warning:** This shell executes AI-generated commands on your system. Commands are shown for review before running (unless you enable yolo mode), but you are responsible for what you execute. Use at your own risk.

An AI-powered Unix shell. Type commands normally — they run via bash. Type plain English — Claude generates the command for you.

```
~/projects > ls -la
total 48
drwxr-xr-x  6 user user 4096 Feb 11 12:00 .
...

~/projects > show me the 10 biggest files in this directory
> du -sh * | sort -rh | head -10
[enter] run / [e]dit / [s]kip
```

## Features

**It's a shell.** Commands execute directly via bash. `cd`, `export`, pipes, redirects, globs — everything works as expected.

**Plain English works too.** If your input isn't a recognized command, Claude generates one. You review it before it runs.

**When things break, it helps.** Failed commands get analyzed. Permission errors automatically offer `sudo` retry. Press `f` after any failure for AI-powered diagnosis.

**Explain anything.** Prefix with `?` to explain a command, or `??` to ask a question:
```
~/projects > ? find . -name "*.log" -mtime +30 -delete
Finds all .log files older than 30 days and deletes them...

~/projects > ?? what's using port 8080
You can check with: lsof -i :8080
```

**Fully customizable.** All AI prompts live in `~/.claudesh/prompts/` as plain text files. Edit the `personality` file to change how the AI responds.

## Requirements

You need all three of these before you start:

1. **Rust toolchain** — install from [rustup.rs](https://rustup.rs/) if you don't have it:
   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Claude CLI** — the `claude` command must be in your PATH. Install from [Anthropic](https://docs.anthropic.com/en/docs/claude-code). The build **will refuse to proceed** without it.
3. **bash** — already present on virtually every Linux and macOS system.

## Install

### Linux / macOS

```sh
git clone https://github.com/gabrielbauman/claudesh.git
cd claudesh
make install
```

This does three things:
1. Checks that `claude` is in your PATH (fails with an error if not)
2. Runs `cargo build --release` to compile the binary
3. Installs the binary to `/usr/local/bin/claudesh`

To install somewhere else, set `PREFIX`:

```sh
make install PREFIX=$HOME/.local    # installs to ~/.local/bin/claudesh
```

To uninstall:

```sh
make uninstall                      # or: make uninstall PREFIX=$HOME/.local
```

### Cargo install (alternative)

If you prefer `cargo install` and have already verified `claude` is available:

```sh
cargo install --path .
```

This puts the binary in `~/.cargo/bin/claudesh`.

### Use as your login shell

Once installed, you can make claudesh your default shell:

```sh
# Add to the system's list of allowed shells
echo /usr/local/bin/claudesh | sudo tee -a /etc/shells

# Set it as your login shell
chsh -s /usr/local/bin/claudesh
```

Adjust the path if you installed to a different `PREFIX`.

## Usage

```
claudesh                     # interactive shell
claudesh -c "ls -la"         # execute a command and exit
claudesh script.sh           # run a script file
echo "ls" | claudesh         # read commands from stdin
claudesh -l                  # login shell (sources ~/.profile)
```

### Interactive commands

| Input | What happens |
|---|---|
| `ls -la` | Runs directly via bash |
| `show me disk usage` | AI generates a command, you confirm |
| `! some command` | Force bash execution (skip AI heuristic) |
| `? tar -xzf foo.tar.gz` | Explains the command |
| `?? how do ssh tunnels work` | Asks the AI a question |
| `cd`, `export`, `unset` | Shell builtins handled natively |
| `judgy` / `judgy on` / `judgy off` | Toggle judgy mode (snarky AI commentary on every command) |
| `yolo` / `yolo on` / `yolo off` | Toggle yolo mode (skip AI command confirmation) |
| `history` | Show command history |
| `exit` / `quit` / Ctrl-D | Exit |

### When a command fails

- **Permission errors** → automatic `sudo` retry offer
- **Any failure** → press `f` for AI diagnosis + suggested fix

### Prompt indicators

```
~/projects >              # normal prompt
~/projects #              # root user
~/projects [1] >          # last command exited with code 1
```

## Configuration

On first run, claudesh creates `~/.claudesh/` with default config files:

```
~/.claudesh/
├── personality            # AI personality (tone, style)
├── claudeshrc             # startup commands (like .bashrc)
├── history                # command history
├── yolo                   # if this file exists, skip confirmation
├── judgy                  # if this file exists, enable judgy mode
└── prompts/
    ├── generate.txt       # command generation from natural language
    ├── explain.txt        # ? command explanations
    ├── ask.txt            # ?? question answering
    ├── fix.txt            # error diagnosis when you press 'f'
    ├── script.txt         # multi-step/complex task generation
    └── judgy.txt          # judgy mode commentary style
```

Every file is plain text. Changes take effect next time claudesh starts.

### Personality

The `personality` file sets the tone for **all** AI responses. It's appended to every prompt sent to Claude. The default is a sardonic Unix veteran, but you can make it anything:

```
You are a grumpy sysadmin who has been doing this since 1987.
You give correct answers but complain about how easy kids have it today.
```

Or make it terse:

```
Maximum 3 lines per response. No filler. Commands only.
```

Or delete the file's contents to get neutral, unadorned responses.

### Prompt files

Each file in `~/.claudesh/prompts/` controls the system prompt for one specific feature:

| File | Used when | What it controls |
|---|---|---|
| `generate.txt` | You type plain English like `show me disk usage` | How Claude turns your request into a shell command |
| `script.txt` | You type something complex like `set up a new react project` | How Claude generates multi-step scripts |
| `explain.txt` | You type `? some-command` | How Claude explains commands |
| `ask.txt` | You type `?? some question` | How Claude answers general questions |
| `fix.txt` | A command fails and you press `f` | How Claude diagnoses errors and suggests fixes |
| `judgy.txt` | Judgy mode is enabled | How Claude generates snarky commentary on your commands |

Edit these to change the AI's behavior for each use case. For example, you could edit `generate.txt` to always prefer `eza` over `ls`, or edit `fix.txt` to always suggest `brew install` instead of `apt install` on your Mac.

### Startup file

`~/.claudesh/claudeshrc` runs on every interactive startup, just like `.bashrc`. Use it for exports or any setup commands:

```sh
export EDITOR=vim
export PATH="$HOME/.local/bin:$PATH"
```

### Judgy mode

When judgy mode is enabled, the AI generates a single sentence of snarky commentary before every command you run. It watches your full session history, so its judgment evolves — repeat a mistake and it will escalate. Commentary appears in dim italics above the command output.

Toggle it at any time from the shell:

```
~/projects > judgy on
judgy mode enabled — I'll be watching.

~/projects > ls
  Oh, listing the directory again. Groundbreaking.
total 48
...

~/projects > judgy off
judgy mode disabled
```

The setting persists across sessions. You can also enable it by creating the file directly:

```sh
touch ~/.claudesh/judgy    # enable
rm ~/.claudesh/judgy       # disable
```

Customize the commentary style by editing `~/.claudesh/prompts/judgy.txt`.

### Yolo mode

By default, when Claude generates a command from natural language, you're asked to confirm before it runs. Yolo mode skips the `[enter] run / [e]dit / [s]kip` confirmation — generated commands execute immediately. The command is still printed so you can see what ran.

Toggle it at any time from the shell:

```
~/projects > yolo on
yolo mode enabled — AI commands run without confirmation

~/projects > yolo off
yolo mode disabled
```

The setting persists across sessions. You can also enable it by creating or removing the file directly:

```sh
touch ~/.claudesh/yolo     # enable
rm ~/.claudesh/yolo        # disable
```

## How command detection works

claudesh decides whether your input is a command or natural language:

1. **Known command** — first word is in `$PATH` or is a shell builtin → runs via bash
2. **Shell syntax** — starts with `/`, `./`, `~`, `$`, `(`, `>`, `sudo`, etc. → runs via bash
3. **Variable assignment** — matches `FOO=bar` pattern → runs via bash
4. **Everything else** → sent to Claude as natural language

Use `!` prefix to force bash if the heuristic gets it wrong.

## Shell contract compliance

claudesh follows standard Unix shell conventions:

- `-c string` — execute command and exit
- Script file execution
- Piped stdin (non-interactive mode)
- Login shell (`-l`, invoked as `-claudesh`)
- Sources `~/.claudesh/claudeshrc` on interactive startup
- Proper exit codes (last command's exit code propagated)
- `#` for root prompt, `>` for regular user
- Ctrl-C / Ctrl-D handling
- `$SHELL`, `$PWD`, `$OLDPWD` set correctly

## Disclaimer

claudesh sends your natural language input to the Claude API to generate shell commands. Those commands run on your machine with your permissions. While claudesh shows generated commands before executing them (unless yolo mode is enabled), AI can produce incorrect or destructive commands. Always review what you're about to run.

This software is provided as-is with no warranty. You are solely responsible for any commands executed through this shell.

## License

MIT
