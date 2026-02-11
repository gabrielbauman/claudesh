# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

claudesh is an AI-powered Unix shell written in Rust. It executes standard commands via bash and sends unrecognized (natural language) input to the Claude CLI for command generation. The entire implementation lives in a single file: `src/main.rs` (~1,600 lines).

## Build & Run Commands

- **Build:** `cargo build --release` (or `make build`, which first checks that `claude` CLI is in PATH)
- **Install:** `make install` (builds + copies binary to `/usr/local/bin/claudesh`)
- **Clean:** `cargo clean`
- **Check only:** `cargo check`
- **Run directly:** `cargo run`

There are no automated tests. The project has no CI/CD configuration.

## Prerequisites

The `claude` CLI (Anthropic's Claude Code) must be in PATH. The Makefile's `check-claude` target enforces this before building. At runtime, claudesh invokes `claude --print --no-input --system <prompt>` and passes user input via stdin.

## Architecture

### Single-file design (`src/main.rs`)

The entire shell is one file with clearly separated sections:

| Section | Approx. Lines | Responsibility |
|---|---|---|
| Constants & config structs | 1–66 | Default prompts (embedded via `include_str!`), ANSI colors, `Config`/`RunResult` structs |
| `main()` + execution modes | 68–315 | Entry point: `-c` flag, script file, piped stdin, or interactive REPL |
| Interactive REPL | 317–540 | Rustyline-based read-eval loop with history, confirmation flow, judgy commentary, error handling |
| Config loading | 542–600 | Reads `~/.claudesh/` files, falls back to embedded defaults |
| Input classification | 546–760 | Classifies input into `InputKind` enum (14 variants) — determines if input is a builtin, shell command, or natural language |
| Bash execution | 762–830 | Spawns `bash -c`, tee's stderr (real-time display + capture for error analysis, capped at 1MB) |
| Shell builtins | 834–1060 | `cd`, `export`, `unset`, `source`, `history`, `judgy`, `yolo` — handled in-process, not delegated to bash |
| Claude integration | 1062–1460 | Calls `claude` CLI for generate/explain/ask/fix/script/judgy operations |
| UI/output helpers | 1462–1607 | Prompt rendering, colored output, help text |

### Input classification flow (`classify_input`)

The heuristic in `classify_input()` (line ~567) determines whether user input is a command or natural language:
1. Comments (`#`), exit/quit, help, history → special handling
2. Mode toggles: `judgy on|off`, `yolo on|off`
3. Prefix-based: `!` (force bash), `??` (ask Claude), `?` (explain command)
4. Shell builtins: cd, export, unset, source
5. `is_shell_command()` check: shell syntax chars (`/`, `~`, `$`, `(`, etc.), variable assignments (`FOO=bar`), command prefixes (`sudo`, `env`, etc.), commands found in PATH
6. Fallback → natural language, sent to Claude

### Claude CLI integration

All AI features invoke the external `claude` binary (not an HTTP API directly). The pattern is:
```
Command::new("claude").args(["--print", "--no-input", "--system", &system_prompt])
```
User input is piped to stdin; raw text output is captured from stdout. The personality prompt is appended to system prompts for explain/ask/fix/judgy but **not** for command generation (to keep generated commands clean).

### Judgy mode

When enabled, the AI generates a single sentence of snarky commentary (displayed in dim italics on stderr) before every command in interactive mode. The full session history (commands and previous commentary) is sent as context so the AI's judgment evolves throughout the session. Toggle via `judgy on|off` builtin or by touching/removing `~/.claudesh/judgy`.

### Embedded defaults (`defaults/` directory)

Prompt templates and personality are embedded at compile time via `include_str!()`. On first run, these are written to `~/.claudesh/` where users can customize them. The `defaults/prompts/` directory contains: `generate.txt`, `explain.txt`, `ask.txt`, `fix.txt`, `script.txt`, `judgy.txt`.

## Dependencies

Only three crates: **rustyline** (line editing + history), **which** (PATH resolution), **dirs** (home directory discovery).
