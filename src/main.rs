use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

// ─── Default prompts (overridable via ~/.claudesh/prompts/) ──────────────────

const DEFAULT_PROMPT_GENERATE: &str = include_str!("../defaults/prompts/generate.txt");
const DEFAULT_PROMPT_EXPLAIN: &str = include_str!("../defaults/prompts/explain.txt");
const DEFAULT_PROMPT_ASK: &str = include_str!("../defaults/prompts/ask.txt");
const DEFAULT_PROMPT_FIX: &str = include_str!("../defaults/prompts/fix.txt");
const DEFAULT_PROMPT_SCRIPT: &str = include_str!("../defaults/prompts/script.txt");
const DEFAULT_PERSONALITY: &str = include_str!("../defaults/personality");

const SHELL_BUILTINS: &[&str] = &[
    "cd", "exit", "quit", "export", "unset", "source", "history", "help", "alias", "unalias",
    "set", "shopt", "type", "hash", "ulimit", "umask", "wait", "jobs", "fg", "bg", "disown",
    "builtin", "command", "declare", "local", "readonly", "typeset", "let", "eval", "exec",
    "trap", "return", "shift", "getopts", "read", "mapfile", "readarray", "printf", "echo",
    "test", "true", "false", "for", "while", "if", "case", "select", "until", "do", "done",
    "then", "else", "elif", "fi", "esac", "in",
];

const COMMAND_PREFIXES: &[&str] = &[
    "sudo ", "env ", "nohup ", "time ", "nice ", "strace ", "watch ", "xargs ",
];

const COLOR_RESET: &str = "\x1b[0m";
const COLOR_BOLD: &str = "\x1b[1m";
const COLOR_DIM: &str = "\x1b[2m";
const COLOR_GREEN: &str = "\x1b[32m";
const COLOR_YELLOW: &str = "\x1b[33m";
const COLOR_MAGENTA: &str = "\x1b[35m";
const COLOR_CYAN: &str = "\x1b[36m";
const COLOR_RED: &str = "\x1b[31m";

/// Loaded configuration from ~/.claudesh/
struct Config {
    prompt_generate: String,
    prompt_explain: String,
    prompt_ask: String,
    prompt_fix: String,
    prompt_script: String,
    personality: String,
    config_dir: PathBuf,
}

/// Result of running a bash command
struct RunResult {
    exit_code: i32,
    captured_stderr: String,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    // Load config
    let config = load_config();

    // Ensure config dir exists with defaults
    ensure_config_dir(&config);

    // Parse arguments for shell contract compliance
    // claudesh -c "command"    → execute command string and exit
    // claudesh script.sh       → execute script file and exit
    // claudesh                  → interactive (or piped stdin)

    let mut arg_idx = 1;
    let mut login_shell = false;

    // Detect login shell (invoked as -claudesh or with -l/--login)
    if args[0].starts_with('-') {
        login_shell = true;
    }

    while arg_idx < args.len() {
        match args[arg_idx].as_str() {
            "-l" | "--login" => {
                login_shell = true;
                arg_idx += 1;
            }
            "-c" => {
                // Execute command string and exit
                if arg_idx + 1 >= args.len() {
                    eprintln!("claudesh: -c: option requires an argument");
                    return ExitCode::from(2);
                }
                let cmd = &args[arg_idx + 1];
                let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
                if login_shell {
                    source_profile(&cwd);
                }
                let result = run_bash(cmd, &cwd);
                return ExitCode::from(result.exit_code as u8);
            }
            "--" => {
                arg_idx += 1;
                break;
            }
            _ => break,
        }
    }

    // If there's a remaining argument, treat it as a script file
    if arg_idx < args.len() {
        let script_path = &args[arg_idx];
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        if login_shell {
            source_profile(&cwd);
        }
        return run_script_file(script_path, &cwd);
    }

    // Set SHELL env var to ourselves
    if let Ok(exe) = env::current_exe() {
        env::set_var("SHELL", &exe);
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    if login_shell {
        source_profile(&cwd);
    }

    // Detect if stdin is a terminal
    let interactive = io::stdin().is_terminal();

    if interactive {
        run_interactive(&config)
    } else {
        run_piped(&config)
    }
}

/// Source profile files for login shells
fn source_profile(cwd: &Path) {
    // Source /etc/profile and ~/.profile via bash, then capture the environment
    let script = r#"
        [ -f /etc/profile ] && . /etc/profile 2>/dev/null
        [ -f ~/.profile ] && . ~/.profile 2>/dev/null
        [ -f ~/.bashrc ] && . ~/.bashrc 2>/dev/null
        env -0
    "#;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .output();

    if let Ok(out) = output {
        let env_str = String::from_utf8_lossy(&out.stdout);
        for entry in env_str.split('\0') {
            if let Some((key, value)) = entry.split_once('=') {
                if !key.is_empty() {
                    env::set_var(key, value);
                }
            }
        }
    }
}

/// Run commands from piped stdin (non-interactive mode)
fn run_piped(config: &Config) -> ExitCode {
    let mut cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    env::set_var("PWD", &cwd);
    let path_commands = build_path_command_set();
    let claude_available = which::which("claude").is_ok();
    let mut last_exit: i32 = 0;

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let input = line.trim();
        if input.is_empty() || input.starts_with('#') {
            continue;
        }

        last_exit = execute_line(input, &mut cwd, &path_commands, claude_available, config, None);
    }

    ExitCode::from(last_exit as u8)
}

/// Run a script file
fn run_script_file(path: &str, cwd: &Path) -> ExitCode {
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("claudesh: {}: {}", path, e);
            return ExitCode::from(127);
        }
    };

    let mut cwd = cwd.to_path_buf();
    env::set_var("PWD", &cwd);
    let path_commands = build_path_command_set();
    let claude_available = which::which("claude").is_ok();
    let config = load_config();
    let mut last_exit: i32 = 0;

    for line in contents.lines() {
        let input = line.trim();
        if input.is_empty() || input.starts_with('#') {
            continue;
        }
        last_exit = execute_line(input, &mut cwd, &path_commands, claude_available, &config, None);
    }

    ExitCode::from(last_exit as u8)
}

/// Execute a single line of input, returns exit code
fn execute_line(
    input: &str,
    cwd: &mut PathBuf,
    path_commands: &HashSet<String>,
    claude_available: bool,
    config: &Config,
    editor: Option<&mut DefaultEditor>,
) -> i32 {
    match classify_input(input, path_commands) {
        InputKind::Exit => std::process::exit(0),
        InputKind::Help => {
            print_help();
            0
        }
        InputKind::Cd(dir) => {
            handle_cd(&dir, cwd);
            0
        }
        InputKind::Export(assignment) => {
            handle_export(&assignment);
            0
        }
        InputKind::Unset(name) => {
            env::remove_var(&name);
            0
        }
        InputKind::History => {
            if let Some(ed) = editor {
                print_history(ed);
            }
            0
        }
        InputKind::ForceBash(cmd) => {
            let result = run_bash(&cmd, cwd);
            result.exit_code
        }
        InputKind::Explain(subject) => {
            if claude_available {
                explain_command(&subject, cwd, config);
            } else {
                eprintln!("{}claude CLI not available{}", COLOR_RED, COLOR_RESET);
            }
            0
        }
        InputKind::Ask(question) => {
            if claude_available {
                ask_question(&question, cwd, config);
            } else {
                eprintln!("{}claude CLI not available{}", COLOR_RED, COLOR_RESET);
            }
            0
        }
        InputKind::ShellCommand(cmd) => {
            let result = run_bash(&cmd, cwd);
            result.exit_code
        }
        InputKind::NaturalLanguage(text) => {
            if claude_available {
                // Non-interactive natural language just generates the command and prints it
                let prompt = build_system_prompt(&config.prompt_generate, &config.personality);
                if let Some(cmd) = call_claude(&prompt, &text, cwd) {
                    let cmd = strip_code_fences(&cmd);
                    println!("{}", cmd);
                }
            } else {
                eprintln!("claudesh: command not found: {}", input);
                return 127;
            }
            0
        }
    }
}

/// Interactive REPL
fn run_interactive(config: &Config) -> ExitCode {
    let mut editor = DefaultEditor::new().expect("Failed to initialize line editor");

    let history_path = history_file_path();
    if let Some(ref path) = history_path {
        let _ = editor.load_history(path);
    }

    let mut cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    env::set_var("PWD", &cwd);

    let path_commands = build_path_command_set();

    let claude_available = which::which("claude").is_ok();
    if !claude_available {
        eprintln!(
            "{}{}warning:{} 'claude' CLI not found in PATH. AI features disabled.",
            COLOR_BOLD, COLOR_YELLOW, COLOR_RESET
        );
    }

    let is_root = is_user_root();
    let mut last_exit: i32 = 0;

    // Source ~/.claudeshrc if it exists
    let rc_path = config.config_dir.join("claudeshrc");
    if rc_path.exists() {
        if let Ok(contents) = fs::read_to_string(&rc_path) {
            for line in contents.lines() {
                let input = line.trim();
                if input.is_empty() || input.starts_with('#') {
                    continue;
                }
                last_exit = execute_line(
                    input,
                    &mut cwd,
                    &path_commands,
                    claude_available,
                    config,
                    Some(&mut editor),
                );
            }
        }
    }

    print_welcome();

    loop {
        let prompt = format_prompt(&cwd, is_root, last_exit);
        match editor.readline(&prompt) {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                editor.add_history_entry(input).ok();

                last_exit = match classify_input(input, &path_commands) {
                    InputKind::Exit => {
                        println!("{}bye{}", COLOR_DIM, COLOR_RESET);
                        break;
                    }
                    InputKind::Help => {
                        print_help();
                        0
                    }
                    InputKind::Cd(dir) => {
                        handle_cd(&dir, &mut cwd);
                        0
                    }
                    InputKind::Export(assignment) => {
                        handle_export(&assignment);
                        0
                    }
                    InputKind::Unset(name) => {
                        env::remove_var(&name);
                        0
                    }
                    InputKind::History => {
                        print_history(&editor);
                        0
                    }
                    InputKind::ForceBash(cmd) => {
                        let result = run_bash(&cmd, &cwd);
                        if result.exit_code != 0 && claude_available {
                            offer_error_help(&cmd, &result, &cwd, &mut editor, config);
                        }
                        result.exit_code
                    }
                    InputKind::Explain(subject) => {
                        if claude_available {
                            explain_command(&subject, &cwd, config);
                        } else {
                            eprintln!("{}claude CLI not available{}", COLOR_RED, COLOR_RESET);
                        }
                        0
                    }
                    InputKind::Ask(question) => {
                        if claude_available {
                            ask_question(&question, &cwd, config);
                        } else {
                            eprintln!("{}claude CLI not available{}", COLOR_RED, COLOR_RESET);
                        }
                        0
                    }
                    InputKind::ShellCommand(cmd) => {
                        let result = run_bash(&cmd, &cwd);
                        if result.exit_code != 0 && claude_available {
                            offer_error_help(&cmd, &result, &cwd, &mut editor, config);
                        }
                        result.exit_code
                    }
                    InputKind::NaturalLanguage(text) => {
                        if claude_available {
                            handle_natural_language_interactive(
                                &text,
                                &cwd,
                                &mut editor,
                                config,
                            )
                        } else {
                            eprintln!(
                                "{}not a recognized command and claude CLI is unavailable{}",
                                COLOR_RED, COLOR_RESET
                            );
                            127
                        }
                    }
                };
            }
            Err(ReadlineError::Interrupted) => {
                println!();
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("{}bye{}", COLOR_DIM, COLOR_RESET);
                break;
            }
            Err(err) => {
                eprintln!("{}error: {:?}{}", COLOR_RED, err, COLOR_RESET);
                break;
            }
        }
    }

    if let Some(ref path) = history_path {
        let _ = editor.save_history(path);
    }

    ExitCode::from(last_exit as u8)
}

// ─── Config ──────────────────────────────────────────────────────────────────

fn load_config() -> Config {
    let config_dir = dirs::home_dir()
        .map(|h| h.join(".claudesh"))
        .unwrap_or_else(|| PathBuf::from(".claudesh"));

    let prompts_dir = config_dir.join("prompts");

    let prompt_generate = load_prompt_file(&prompts_dir, "generate.txt", DEFAULT_PROMPT_GENERATE);
    let prompt_explain = load_prompt_file(&prompts_dir, "explain.txt", DEFAULT_PROMPT_EXPLAIN);
    let prompt_ask = load_prompt_file(&prompts_dir, "ask.txt", DEFAULT_PROMPT_ASK);
    let prompt_fix = load_prompt_file(&prompts_dir, "fix.txt", DEFAULT_PROMPT_FIX);
    let prompt_script = load_prompt_file(&prompts_dir, "script.txt", DEFAULT_PROMPT_SCRIPT);
    let personality = load_prompt_file(&config_dir, "personality", DEFAULT_PERSONALITY);

    Config {
        prompt_generate,
        prompt_explain,
        prompt_ask,
        prompt_fix,
        prompt_script,
        personality,
        config_dir,
    }
}

fn load_prompt_file(dir: &Path, filename: &str, default: &str) -> String {
    let path = dir.join(filename);
    fs::read_to_string(&path)
        .unwrap_or_else(|_| default.to_string())
        .trim()
        .to_string()
}

/// Create ~/.claudesh/ with default files if it doesn't exist
fn ensure_config_dir(config: &Config) {
    let dir = &config.config_dir;
    let prompts_dir = dir.join("prompts");

    if !dir.exists() {
        fs::create_dir_all(&prompts_dir).ok();

        // Write default files
        write_default(dir, "personality", DEFAULT_PERSONALITY);
        write_default(&prompts_dir, "generate.txt", DEFAULT_PROMPT_GENERATE);
        write_default(&prompts_dir, "explain.txt", DEFAULT_PROMPT_EXPLAIN);
        write_default(&prompts_dir, "ask.txt", DEFAULT_PROMPT_ASK);
        write_default(&prompts_dir, "fix.txt", DEFAULT_PROMPT_FIX);
        write_default(&prompts_dir, "script.txt", DEFAULT_PROMPT_SCRIPT);
    }
}

fn write_default(dir: &Path, filename: &str, content: &str) {
    let path = dir.join(filename);
    if !path.exists() {
        fs::write(&path, content).ok();
    }
}

fn build_system_prompt(base_prompt: &str, personality: &str) -> String {
    if personality.is_empty() {
        base_prompt.to_string()
    } else {
        format!("{}\n\nPersonality: {}", base_prompt, personality)
    }
}

// ─── Input Classification ────────────────────────────────────────────────────

#[derive(Debug)]
enum InputKind {
    Exit,
    Help,
    Cd(String),
    Export(String),
    Unset(String),
    History,
    ForceBash(String),
    Explain(String),
    Ask(String),
    ShellCommand(String),
    NaturalLanguage(String),
}

fn classify_input(input: &str, path_commands: &HashSet<String>) -> InputKind {
    if input == "exit" || input == "quit" || input == "logout" {
        return InputKind::Exit;
    }
    if input == "help" {
        return InputKind::Help;
    }
    if input == "history" {
        return InputKind::History;
    }

    // ! prefix: force bash execution
    if let Some(cmd) = input.strip_prefix("! ").or_else(|| input.strip_prefix("!")) {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            return InputKind::ForceBash(cmd.to_string());
        }
    }

    // ?? prefix: ask a question (check before single ?)
    if let Some(question) = input.strip_prefix("?? ").or_else(|| input.strip_prefix("??")) {
        let question = question.trim();
        if !question.is_empty() {
            return InputKind::Ask(question.to_string());
        }
    }

    // ? prefix: explain a command
    if let Some(subject) = input.strip_prefix("? ").or_else(|| input.strip_prefix("?")) {
        let subject = subject.trim();
        if !subject.is_empty() {
            return InputKind::Explain(subject.to_string());
        }
    }

    // cd builtin
    if input == "cd" || input == "cd " {
        return InputKind::Cd(String::new());
    }
    if let Some(dir) = input.strip_prefix("cd ") {
        return InputKind::Cd(dir.trim().to_string());
    }

    // export builtin
    if let Some(assignment) = input.strip_prefix("export ") {
        return InputKind::Export(assignment.trim().to_string());
    }

    // unset builtin
    if let Some(name) = input.strip_prefix("unset ") {
        return InputKind::Unset(name.trim().to_string());
    }

    // Check if it looks like a shell command
    if is_shell_command(input, path_commands) {
        InputKind::ShellCommand(input.to_string())
    } else {
        InputKind::NaturalLanguage(input.to_string())
    }
}

fn is_shell_command(input: &str, path_commands: &HashSet<String>) -> bool {
    let first_char = input.chars().next().unwrap_or(' ');

    // Shell syntax characters
    if matches!(
        first_char,
        '/' | '.' | '~' | '(' | '{' | '[' | '$' | '<' | '>' | '#'
    ) {
        return true;
    }

    // Variable assignment: FOO=bar
    if let Some(eq_pos) = input.find('=') {
        let before_eq = &input[..eq_pos];
        if !before_eq.is_empty()
            && !before_eq.contains(' ')
            && before_eq
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return true;
        }
    }

    // Command prefixes: sudo, env, nohup, etc.
    for prefix in COMMAND_PREFIXES {
        if input.starts_with(prefix) {
            return true;
        }
    }

    // Get first token (handle pipes, semicolons, &&)
    let first_token = input.split_whitespace().next().unwrap_or("");
    let first_token = first_token.split('|').next().unwrap_or(first_token);
    let first_token = first_token.split(';').next().unwrap_or(first_token);
    let first_token = first_token.split('&').next().unwrap_or(first_token);

    // Shell builtins
    if SHELL_BUILTINS.contains(&first_token) {
        return true;
    }

    // Commands in PATH
    if path_commands.contains(first_token) {
        return true;
    }

    // Path to executable
    if first_token.contains('/') {
        return true;
    }

    false
}

fn build_path_command_set() -> HashSet<String> {
    let mut commands = HashSet::new();
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(':') {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        commands.insert(name.to_string());
                    }
                }
            }
        }
    }
    commands
}

// ─── Bash Execution ──────────────────────────────────────────────────────────

/// Run a command via bash with inherited stdin/stdout.
/// Stderr is tee'd: displayed in real-time AND captured for error analysis.
fn run_bash(cmd: &str, cwd: &Path) -> RunResult {
    let child = Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            let stderr_pipe = child.stderr.take().unwrap();
            let stderr_thread = std::thread::spawn(move || {
                let mut captured = String::new();
                let reader = BufReader::new(stderr_pipe);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            eprintln!("{}", line);
                            captured.push_str(&line);
                            captured.push('\n');
                        }
                        Err(_) => break,
                    }
                }
                captured
            });

            let status = child.wait();
            let captured_stderr = stderr_thread.join().unwrap_or_default();

            let exit_code = match status {
                Ok(s) => s.code().unwrap_or(1),
                Err(_) => 1,
            };

            RunResult {
                exit_code,
                captured_stderr,
            }
        }
        Err(e) => {
            let msg = format!("failed to execute: {}", e);
            eprintln!("{}{}{}", COLOR_RED, msg, COLOR_RESET);
            RunResult {
                exit_code: 127,
                captured_stderr: msg,
            }
        }
    }
}

// ─── Builtins ────────────────────────────────────────────────────────────────

fn handle_cd(dir: &str, cwd: &mut PathBuf) {
    let target = if dir.is_empty() {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    } else if dir == "-" {
        if let Ok(old) = env::var("OLDPWD") {
            println!("{}", old);
            PathBuf::from(old)
        } else {
            eprintln!("{}cd: OLDPWD not set{}", COLOR_RED, COLOR_RESET);
            return;
        }
    } else {
        let expanded = shellexpand_tilde(dir);
        let path = Path::new(&expanded);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        }
    };

    match target.canonicalize() {
        Ok(real_path) => {
            if real_path.is_dir() {
                env::set_var("OLDPWD", cwd.as_os_str());
                *cwd = real_path.clone();
                env::set_current_dir(&real_path).ok();
                env::set_var("PWD", &real_path);
            } else {
                eprintln!(
                    "{}cd: not a directory: {}{}",
                    COLOR_RED,
                    target.display(),
                    COLOR_RESET
                );
            }
        }
        Err(_) => {
            eprintln!(
                "{}cd: no such directory: {}{}",
                COLOR_RED,
                target.display(),
                COLOR_RESET
            );
        }
    }
}

fn handle_export(assignment: &str) {
    if let Some((key, value)) = assignment.split_once('=') {
        let key = key.trim();
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
            })
            .unwrap_or(value);
        env::set_var(key, value);
    } else {
        // `export VAR` without = is a no-op since env is inherited
        eprintln!(
            "{}(variable already exported to child processes){}",
            COLOR_DIM, COLOR_RESET
        );
    }
}

fn print_history(editor: &DefaultEditor) {
    for (i, entry) in editor.history().iter().enumerate() {
        println!("  {}{:4}{} {}", COLOR_DIM, i + 1, COLOR_RESET, entry);
    }
}

fn shellexpand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.display().to_string();
        }
    }
    path.to_string()
}

fn is_user_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}

// ─── Claude Integration ──────────────────────────────────────────────────────

fn call_claude(system_prompt: &str, user_message: &str, cwd: &Path) -> Option<String> {
    let context = format!(
        "Current directory: {}\nOS: {}\nShell: claudesh\nUser: {}\n\nUser input: {}",
        cwd.display(),
        std::env::consts::OS,
        env::var("USER").unwrap_or_else(|_| "unknown".into()),
        user_message
    );

    let output = Command::new("claude")
        .arg("--print")
        .arg("--no-input")
        .arg("--system")
        .arg(system_prompt)
        .arg(&context)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                eprintln!("{}claude error: {}{}", COLOR_RED, err.trim(), COLOR_RESET);
                None
            }
        }
        Err(e) => {
            eprintln!("{}failed to run claude: {}{}", COLOR_RED, e, COLOR_RESET);
            None
        }
    }
}

fn handle_natural_language_interactive(
    text: &str,
    cwd: &Path,
    editor: &mut DefaultEditor,
    config: &Config,
) -> i32 {
    let lower = text.to_lowercase();
    let is_complex = lower.contains(" and then ")
        || lower.contains(" step by step")
        || lower.contains("script")
        || lower.contains("automate")
        || lower.contains("set up")
        || lower.contains("setup")
        || lower.contains("install and configure")
        || lower.contains("create a project");

    let base_prompt = if is_complex {
        &config.prompt_script
    } else {
        &config.prompt_generate
    };

    let prompt = build_system_prompt(base_prompt, &config.personality);

    eprint!(
        "{}{}thinking...{}",
        COLOR_DIM, COLOR_MAGENTA, COLOR_RESET
    );

    let generated = call_claude(&prompt, text, cwd);

    eprint!("\r{}\r", " ".repeat(40));

    match generated {
        Some(cmd) => {
            let cmd = strip_code_fences(&cmd);
            println!(
                "{}{}>{} {}",
                COLOR_BOLD, COLOR_CYAN, COLOR_RESET, cmd
            );

            eprint!(
                "{}[enter] run / [e]dit / [s]kip{} ",
                COLOR_DIM, COLOR_RESET
            );
            io::stderr().flush().ok();

            let choice = read_single_line().trim().to_lowercase();
            match choice.as_str() {
                "" | "r" | "run" | "y" | "yes" => {
                    editor.add_history_entry(&cmd).ok();
                    let result = run_bash(&cmd, cwd);
                    if result.exit_code != 0 {
                        offer_error_help(&cmd, &result, cwd, editor, config);
                    }
                    result.exit_code
                }
                "e" | "edit" => {
                    eprint!("{}> {}", COLOR_YELLOW, COLOR_RESET);
                    io::stderr().flush().ok();
                    let edited = read_single_line();
                    let edited = edited.trim();
                    if !edited.is_empty() {
                        editor.add_history_entry(edited).ok();
                        let result = run_bash(edited, cwd);
                        if result.exit_code != 0 {
                            offer_error_help(edited, &result, cwd, editor, config);
                        }
                        result.exit_code
                    } else {
                        0
                    }
                }
                _ => {
                    eprintln!("{}skipped{}", COLOR_DIM, COLOR_RESET);
                    0
                }
            }
        }
        None => {
            eprintln!(
                "{}couldn't generate a command for that{}",
                COLOR_RED, COLOR_RESET
            );
            1
        }
    }
}

fn explain_command(subject: &str, cwd: &Path, config: &Config) {
    let prompt = build_system_prompt(&config.prompt_explain, &config.personality);

    eprint!(
        "{}{}thinking...{}",
        COLOR_DIM, COLOR_MAGENTA, COLOR_RESET
    );

    let explanation = call_claude(&prompt, subject, cwd);

    eprint!("\r{}\r", " ".repeat(40));

    match explanation {
        Some(text) => {
            println!("{}{}{}", COLOR_GREEN, text, COLOR_RESET);
        }
        None => {
            eprintln!("{}couldn't explain that{}", COLOR_RED, COLOR_RESET);
        }
    }
}

fn ask_question(question: &str, cwd: &Path, config: &Config) {
    let prompt = build_system_prompt(&config.prompt_ask, &config.personality);

    eprint!(
        "{}{}thinking...{}",
        COLOR_DIM, COLOR_MAGENTA, COLOR_RESET
    );

    let answer = call_claude(&prompt, question, cwd);

    eprint!("\r{}\r", " ".repeat(40));

    match answer {
        Some(text) => {
            println!("{}{}{}", COLOR_GREEN, text, COLOR_RESET);
        }
        None => {
            eprintln!("{}couldn't answer that{}", COLOR_RED, COLOR_RESET);
        }
    }
}

/// Handle a failed command: detect permission errors (offer sudo), or use AI
fn offer_error_help(
    cmd: &str,
    result: &RunResult,
    cwd: &Path,
    editor: &mut DefaultEditor,
    config: &Config,
) {
    let stderr = &result.captured_stderr;
    let exit_code = result.exit_code;

    // Quick-detect permission errors
    let is_permission_error = stderr.contains("Permission denied")
        || stderr.contains("permission denied")
        || stderr.contains("EACCES")
        || stderr.contains("Operation not permitted")
        || stderr.contains("must be root")
        || stderr.contains("Access denied");

    if is_permission_error && !cmd.starts_with("sudo ") {
        eprint!(
            "{}permission denied{} — retry with {}sudo{}? [y/N] ",
            COLOR_RED, COLOR_RESET, COLOR_YELLOW, COLOR_RESET,
        );
        io::stderr().flush().ok();

        let choice = read_single_line().trim().to_lowercase();
        if choice == "y" || choice == "yes" {
            let sudo_cmd = format!("sudo {}", cmd);
            editor.add_history_entry(&sudo_cmd).ok();
            let retry = run_bash(&sudo_cmd, cwd);
            if retry.exit_code != 0 {
                eprint!(
                    "{}exit code {}{} — press {}f{} for AI help ",
                    COLOR_RED, retry.exit_code, COLOR_RESET, COLOR_YELLOW, COLOR_RESET
                );
                io::stderr().flush().ok();
                let choice = read_single_line().trim().to_lowercase();
                if choice == "f" {
                    do_ai_error_analysis(cmd, &retry.captured_stderr, retry.exit_code, cwd, editor, config);
                }
            }
            return;
        }
    }

    eprint!(
        "{}exit {}{}{} — press {}f{} for AI help or enter to continue ",
        COLOR_DIM, COLOR_RED, exit_code, COLOR_RESET, COLOR_YELLOW, COLOR_RESET
    );
    io::stderr().flush().ok();

    let choice = read_single_line().trim().to_lowercase();
    if choice == "f" || choice == "fix" {
        do_ai_error_analysis(cmd, stderr, exit_code, cwd, editor, config);
    }
}

fn do_ai_error_analysis(
    cmd: &str,
    stderr: &str,
    exit_code: i32,
    cwd: &Path,
    editor: &mut DefaultEditor,
    config: &Config,
) {
    let error_context = format!(
        "Command: {}\nExit code: {}\nStderr:\n{}",
        cmd, exit_code, stderr
    );

    let prompt = build_system_prompt(&config.prompt_fix, &config.personality);

    eprint!(
        "{}{}analyzing...{}",
        COLOR_DIM, COLOR_MAGENTA, COLOR_RESET
    );

    let help = call_claude(&prompt, &error_context, cwd);

    eprint!("\r{}\r", " ".repeat(40));

    if let Some(text) = help {
        let text = strip_code_fences(&text);
        // Try to split into explanation + suggested command
        let parts: Vec<&str> = text.splitn(2, "\n\n").collect();
        if parts.len() == 2 {
            let explanation = parts[0].trim();
            let suggested_cmd = parts[1].trim();

            eprintln!("{}{}{}", COLOR_YELLOW, explanation, COLOR_RESET);
            println!(
                "{}{}>{} {}",
                COLOR_BOLD, COLOR_CYAN, COLOR_RESET, suggested_cmd
            );

            eprint!(
                "{}[enter] run / [s]kip{} ",
                COLOR_DIM, COLOR_RESET
            );
            io::stderr().flush().ok();

            let choice = read_single_line().trim().to_lowercase();
            if choice.is_empty() || choice == "r" || choice == "y" || choice == "run" {
                editor.add_history_entry(suggested_cmd).ok();
                run_bash(suggested_cmd, cwd);
            }
        } else {
            eprintln!("{}{}{}", COLOR_YELLOW, text, COLOR_RESET);
        }
    }
}

fn strip_code_fences(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("```") {
        let s = s
            .trim_start_matches("```bash")
            .trim_start_matches("```sh")
            .trim_start_matches("```shell")
            .trim_start_matches("```");
        let s = if let Some(idx) = s.rfind("```") {
            &s[..idx]
        } else {
            s
        };
        return s.trim().to_string();
    }
    s.to_string()
}

// ─── Utilities ───────────────────────────────────────────────────────────────

fn read_single_line() -> String {
    let mut line = String::new();
    let stdin = io::stdin();
    stdin.lock().read_line(&mut line).ok();
    line
}

fn format_prompt(cwd: &Path, is_root: bool, last_exit: i32) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let display_path = if let Ok(relative) = cwd.strip_prefix(&home) {
        if relative.as_os_str().is_empty() {
            "~".to_string()
        } else {
            format!("~/{}", relative.display())
        }
    } else {
        cwd.display().to_string()
    };

    let sigil = if is_root { "#" } else { ">" };

    // Show last exit code in red if non-zero
    let status_indicator = if last_exit != 0 {
        format!(" {}[{}]{}", COLOR_RED, last_exit, COLOR_RESET)
    } else {
        String::new()
    };

    format!(
        "{}{}{} {}{}{}{} ",
        COLOR_MAGENTA,
        display_path,
        status_indicator,
        COLOR_CYAN,
        COLOR_BOLD,
        sigil,
        COLOR_RESET,
    )
}

fn history_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claudesh").join("history"))
}

fn print_welcome() {
    println!(
        "\n  {}{}claudesh{} — AI-powered shell",
        COLOR_BOLD, COLOR_MAGENTA, COLOR_RESET
    );
    println!(
        "  {}type commands normally, or just say what you want in plain English{}",
        COLOR_DIM, COLOR_RESET
    );
    println!(
        "  {}type{} help {}for more info{}\n",
        COLOR_DIM, COLOR_RESET, COLOR_DIM, COLOR_RESET
    );
}

fn print_help() {
    println!(
        r#"
  {b}{m}claudesh{r} — AI-powered Unix shell

  {b}Usage:{r}
    {g}any command{r}           run it directly via bash
    {g}plain english{r}         AI generates a command, you confirm
    {y}! command{r}             force bash execution (bypass heuristic)
    {y}? command{r}             explain what a command does
    {y}?? question{r}           ask the AI anything

  {b}When a command fails:{r}
    {y}sudo auto-detect{r}      permission errors offer sudo retry
    press {y}f{r}               AI-powered error diagnosis + suggested fix

  {b}After AI generates a command:{r}
    {y}enter{r}                 run it
    {y}e{r}                     edit before running
    {y}s{r} / anything else     skip

  {b}Builtins:{r}
    {g}cd{r} {d}[dir]{r}              change directory ({g}cd -{r} for previous)
    {g}export{r} {d}KEY=VALUE{r}      set environment variable
    {g}unset{r} {d}VAR{r}             remove environment variable
    {g}history{r}               show command history
    {g}exit{r} / {g}quit{r}            exit the shell
    {g}help{r}                  this message

  {b}Shell modes:{r}
    {d}claudesh{r}               interactive shell
    {d}claudesh -c "cmd"{r}      execute a command and exit
    {d}claudesh script.sh{r}     run a script file
    {d}echo "cmd" | claudesh{r}  read commands from stdin
    {d}claudesh -l{r}            login shell (sources profile)

  {b}Configuration:{r}  {d}~/.claudesh/{r}
    {d}personality{r}            customize AI personality
    {d}prompts/*.txt{r}          override AI system prompts
    {d}claudeshrc{r}             startup commands (like .bashrc)
    {d}history{r}                command history

  {b}Examples:{r}
    {d}$ ls -la{r}                                 {d}# just runs{r}
    {d}$ git log --oneline -10{r}                  {d}# just runs{r}
    {d}$ show me the biggest files here{r}          {d}# AI generates command{r}
    {d}$ find all TODOs in the source code{r}       {d}# AI generates command{r}
    {d}$ ? tar -xzf archive.tar.gz{r}              {d}# explains the command{r}
    {d}$ ?? how do I forward a port over ssh{r}     {d}# asks AI a question{r}
    {d}$ set up a new react project{r}              {d}# AI generates script{r}
"#,
        b = COLOR_BOLD,
        r = COLOR_RESET,
        d = COLOR_DIM,
        g = COLOR_GREEN,
        y = COLOR_YELLOW,
        m = COLOR_MAGENTA,
    );
}
