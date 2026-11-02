//! Installed desktop entries are parsed as argv, never evaluated by a shell.
use crate::{config::valid_environment_key, model::*};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledLauncher {
    pub id: String,
    pub name: String,
    pub path: String,
    pub profile: Option<Profile>,
    pub problem: Option<String>,
}

pub fn executable_path(program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let usable = |p: &Path| {
        fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = if program.contains('/') {
        let path = PathBuf::from(program);
        usable(&path).then_some(path)
    } else {
        env::var_os("PATH").and_then(|path| {
            env::split_paths(&path)
                .map(|p| p.join(program))
                .find(|p| usable(p))
        })
    }?;
    Some(fs::canonicalize(&path).unwrap_or(path))
}

pub fn same_executable(left: &str, right: &str) -> bool {
    // /proc/exe appends this suffix when a running binary has been upgraded.
    let left = left.strip_suffix(" (deleted)").unwrap_or(left);
    let right = right.strip_suffix(" (deleted)").unwrap_or(right);
    left == right
        || executable_path(left)
            .zip(executable_path(right))
            .is_some_and(|(a, b)| a == b)
}

pub fn installed() -> Vec<InstalledLauncher> {
    let mut roots = Vec::new();
    if let Some(data) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    {
        roots.push(data.join("applications"));
    }
    roots.extend(
        env::split_paths(
            &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        )
        .filter(|p| p.is_absolute())
        .map(|p| p.join("applications")),
    );
    discover(&roots)
}

pub fn discover(roots: &[PathBuf]) -> Vec<InstalledLauncher> {
    fn files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        if depth > 8 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                files(root, &path, depth + 1, out);
            } else if path.extension().is_some_and(|x| x == "desktop") {
                if let Ok(relative) = path.strip_prefix(root) {
                    out.push((relative.to_string_lossy().replace('/', "-"), path));
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for root in roots {
        let mut entries = Vec::new();
        files(root, root, 0, &mut entries);
        entries.sort();
        for (id, path) in entries {
            // Higher-priority Hidden entries mask their system counterparts.
            if !seen.insert(id.clone()) || id.starts_with("igpu-man-") || id == "igpu-man.desktop" {
                continue;
            }
            if fs::metadata(&path).is_ok_and(|m| m.len() > 128 * 1024) {
                continue;
            }
            let Ok(raw) = crate::fsutil::read_text_limited(&path, 128 * 1024) else {
                continue;
            };
            if let Some(entry) = parse_entry(&id, &path, &raw) {
                result.push(entry);
            }
        }
    }
    result.sort_by_key(|e| (e.name.to_lowercase(), e.id.clone()));
    result
}

fn unescape(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => result.push(' '),
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some('r') => result.push('\r'),
            Some('\\') => result.push('\\'),
            Some(c) => {
                result.push('\\');
                result.push(c);
            }
            None => result.push('\\'),
        }
    }
    result
}

fn parse_entry(id: &str, path: &Path, raw: &str) -> Option<InstalledLauncher> {
    let mut fields = BTreeMap::new();
    let mut main = false;
    for line in raw.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        if !main || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let field = |key: &str| fields.get(key).map(String::as_str).unwrap_or("");
    if field("Hidden") == "true" || field("NoDisplay") == "true" || field("Type") != "Application" {
        return None;
    }
    let desktops: Vec<_> = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .map(str::to_owned)
        .collect();
    if !field("OnlyShowIn").is_empty()
        && !field("OnlyShowIn")
            .split(';')
            .any(|d| !d.is_empty() && desktops.iter().any(|v| v == d))
    {
        return None;
    }
    if field("NotShowIn")
        .split(';')
        .any(|d| !d.is_empty() && desktops.iter().any(|v| v == d))
    {
        return None;
    }
    let locale = env::var("LC_MESSAGES")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| env::var("LANG").ok())
        .unwrap_or_default();
    let locale = locale.split('.').next().unwrap_or("");
    let name = [
        format!("Name[{locale}]"),
        format!("Name[{}]", locale.split('_').next().unwrap_or("")),
        "Name".into(),
    ]
    .into_iter()
    .find_map(|key| fields.get(&key))
    .map(|v| unescape(v))
    .unwrap_or_else(|| id.to_owned());
    let profile = (|| -> Result<Profile> {
        if field("Terminal") == "true" {
            bail!("Terminal launchers need a manually configured profile");
        }
        if !field("TryExec").is_empty() && executable_path(&unescape(field("TryExec"))).is_none() {
            bail!("TryExec program is not installed");
        }
        import_exec(
            id,
            &name,
            path,
            field("Exec"),
            &unescape(field("Icon")),
            field("MimeType"),
            field("Categories"),
            &unescape(field("Path")),
        )
    })();
    let (profile, problem) = match profile {
        Ok(p) => (Some(p), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    Some(InstalledLauncher {
        id: id.into(),
        name,
        path: path.to_string_lossy().into(),
        profile,
        problem,
    })
}

// Apply desktop string escaping before Exec quoting, per the desktop-entry spec.
fn tokenize(raw: &str) -> Result<Vec<(String, bool)>> {
    let raw = unescape(raw);
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut started = false;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                was_quoted = true;
                started = true;
            }
            '\\' => {
                let next = chars.next().context("Trailing backslash in Exec")?;
                if quoted && !['"', '`', '$', '\\'].contains(&next) {
                    bail!("Unsupported quoted Exec escape");
                }
                token.push(next);
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    tokens.push((std::mem::take(&mut token), was_quoted));
                }
                was_quoted = false;
                started = false;
            }
            c => {
                if !quoted && "'<>~|&;$*?#()`".contains(c) {
                    bail!("Reserved Exec characters must use desktop-entry double quoting");
                }
                token.push(c);
                started = true;
            }
        }
    }
    if quoted {
        bail!("Unclosed quote in Exec");
    }
    if started {
        tokens.push((token, was_quoted));
    }
    if tokens.is_empty() {
        bail!("Launcher has no Exec command");
    }
    Ok(tokens)
}

#[allow(clippy::too_many_arguments)]
fn import_exec(
    id: &str,
    name: &str,
    path: &Path,
    raw: &str,
    icon: &str,
    mime_types: &str,
    categories: &str,
    cwd: &str,
) -> Result<Profile> {
    let tokens = tokenize(raw)?;
    let mut args = Vec::new();
    let mut file_argument = None;
    for (token, quoted) in tokens {
        if quoted && token.replace("%%", "").contains('%') {
            bail!("Field codes inside quoted Exec arguments are unsupported");
        }
        if ["%f", "%F", "%u", "%U"].contains(&token.as_str()) {
            if file_argument.is_some() {
                bail!("Multiple file/URL field codes are invalid");
            }
            file_argument = Some((args.len(), token.chars().nth(1).unwrap()));
            continue;
        }
        if token == "%i" {
            if !icon.is_empty() {
                args.extend(["--icon".into(), icon.into()]);
            }
            continue;
        }
        let mut expanded = String::new();
        let mut chars = token.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                expanded.push(c);
                continue;
            }
            match chars.next().context("Incomplete Exec field code")? {
                '%' => expanded.push('%'),
                'c' => expanded.push_str(name),
                'k' => expanded.push_str(&path.to_string_lossy()),
                'd' | 'D' | 'n' | 'N' | 'v' | 'm' => {}
                _ => bail!("Unsupported or embedded Exec field code"),
            }
        }
        if !expanded.is_empty() || token.is_empty() {
            args.push(expanded);
        }
    }
    if args.is_empty() || file_argument.is_some_and(|(i, _)| i == 0) {
        bail!("Launcher has no executable before file arguments");
    }
    let mut environment = BTreeMap::new();
    let mut prefix = 1;
    if Path::new(&args[0]).file_name().is_some_and(|n| n == "env") {
        while let Some(arg) = args.get(prefix) {
            let Some((key, value)) = arg.split_once('=') else {
                break;
            };
            if !valid_environment_key(key) {
                bail!("Unsupported env assignment in launcher");
            }
            environment.insert(key.into(), value.into());
            prefix += 1;
        }
        if args.get(prefix).is_none_or(|s| s.starts_with('-')) {
            bail!("env options require a manually configured profile");
        }
        prefix += 1;
    }
    let executable = args[prefix - 1].clone();
    let base = Path::new(&executable)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    if ["sh", "bash", "zsh", "fish", "flatpak", "snap"].contains(&base.as_ref()) {
        bail!("Shell and sandbox launchers require a separately configured workflow");
    }
    if executable_path(&executable).is_none() {
        bail!("Launcher executable is not installed or executable");
    }
    let file_argument = match file_argument {
        Some((index, code)) if index >= prefix => Some((index - prefix, code)),
        Some(_) => bail!("File field code appears before the executable"),
        None => None,
    };
    let profile = Profile {
        name: "imported".into(),
        firefox: crate::launch::is_firefox(&executable),
        executable,
        args: args[prefix..].to_vec(),
        gpu: "igpu".into(),
        environment,
        working_directory: (!cwd.is_empty()).then(|| cwd.into()),
        desktop: Some(DesktopMetadata {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
            mime_types: mime_types.into(),
            categories: categories.into(),
            file_argument,
        }),
        ..Profile::default()
    };
    crate::config::validate_profile(&profile)?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(exec: &str) -> Result<Profile> {
        import_exec(
            "test.desktop",
            "Test %U",
            Path::new("/tmp/test.desktop"),
            exec,
            "test-icon",
            "",
            "",
            "",
        )
    }
    #[test]
    fn imports_quoted_arguments_env_and_field_positions_without_shell_evaluation() {
        let p = parse(
            r#"env TEST_MODE="two words" /usr/bin/printf "$(touch /tmp/NEVER)" %% %c %i %U suffix"#,
        )
        .unwrap();
        assert_eq!(p.executable, "/usr/bin/printf");
        assert_eq!(p.environment["TEST_MODE"], "two words");
        assert_eq!(
            p.args,
            [
                "$(touch /tmp/NEVER)",
                "%",
                "Test %U",
                "--icon",
                "test-icon",
                "suffix"
            ]
        );
        assert_eq!(p.desktop.unwrap().file_argument, Some((5, 'U')));
    }
    #[test]
    fn rejects_malformed_or_unsupported_launchers() {
        for exec in [
            "/usr/bin/printf %Z",
            "/usr/bin/printf \"%U\"",
            "/usr/bin/printf %u %F",
            "/usr/bin/printf --file=%U",
            "/usr/bin/printf \"oops",
            "env -u FOO /usr/bin/printf",
            "sh -c /usr/bin/printf",
            "/usr/bin/printf ; touch /tmp/NO",
        ] {
            assert!(parse(exec).is_err(), "{exec}");
        }
    }
    #[test]
    fn user_hidden_override_and_nested_ids_take_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user");
        let system = temp.path().join("system");
        fs::create_dir_all(user.join("nested")).unwrap();
        fs::create_dir_all(&system).unwrap();
        fs::write(
            user.join("hidden.desktop"),
            "[Desktop Entry]\nHidden=true\n",
        )
        .unwrap();
        fs::write(
            system.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=/usr/bin/printf\n",
        )
        .unwrap();
        fs::write(user.join("nested/app.desktop"), "[Desktop Entry]\nType=Application\nName=User App\nExec=/usr/bin/printf %U\nPath=/tmp\n").unwrap();
        fs::write(
            system.join("nested-app.desktop"),
            "[Desktop Entry]\nType=Application\nName=System App\nExec=/usr/bin/printf\n",
        )
        .unwrap();
        let entries = discover(&[user, system]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "nested-app.desktop");
        assert_eq!(entries[0].name, "User App");
        assert_eq!(
            entries[0]
                .profile
                .as_ref()
                .unwrap()
                .working_directory
                .as_deref(),
            Some("/tmp")
        );
    }
    #[test]
    fn double_escaping_preserves_literal_backslashes() {
        let p = parse(r#"/usr/bin/printf "a\\\\b" """#).unwrap();
        assert_eq!(p.args, [r"a\b", ""]);
    }
}
