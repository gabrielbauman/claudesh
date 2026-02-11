# claudesh

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

- [Rust toolchain](https://rustup.rs/) (for building)
- [Claude CLI](https://docs.anthropic.com/en/docs/claude-code) (`claude` in PATH) — **required, build will fail without it**
- bash

## Install

```sh
git clone https://github.com/gabrielbauman/claudesh.git
cd claudesh
make install
```

The build checks that the `claude` CLI is in your PATH and refuses to proceed without it. This is intentional — claudesh without Claude is just bash with extra steps.

Install to `/usr/local/bin` (default) or override with `PREFIX`:

```sh
make install PREFIX=$HOME/.local
```

### Use as your login shell

```sh
# Add to allowed shells
echo /usr/local/bin/claudesh | sudo tee -a /etc/shells

# Set as your shell
chsh -s /usr/local/bin/claudesh
```

### Cargo install (alternative)

```sh
cargo install --path .
```

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
| `history` | Show command history |
| `exit` / `quit` / Ctrl-D | Exit |

### When a command fails

- **Permission errors** → automatic `sudo` retry offer
- **Any failure** → press `f` for AI diagnosis + suggested fix

### Prompt indicators

```
~/projects >              # normal prompt
~/projects # >            # root user
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
└── prompts/
    ├── generate.txt       # prompt for generating commands
    ├── explain.txt        # prompt for explaining commands
    ├── ask.txt            # prompt for answering questions
    ├── fix.txt            # prompt for error diagnosis
    └── script.txt         # prompt for multi-step scripts
```

Edit any file to customize behavior. For example, change `personality` to:

```
You are a grumpy sysadmin who has been doing this since 1987.
You give correct answers but complain about how easy kids have it today.
```

Or make it terse:

```
Maximum 3 lines per response. No filler. Commands only.
```

### Yolo mode

By default, when Claude generates a command from natural language, you're asked to confirm before it runs. If you trust the AI and want to live dangerously:

```sh
touch ~/.claudesh/yolo
```

This skips the `[enter] run / [e]dit / [s]kip` confirmation — generated commands execute immediately. The command is still printed so you can see what ran. Remove the file to go back to normal:

```sh
rm ~/.claudesh/yolo
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

## License

MIT
