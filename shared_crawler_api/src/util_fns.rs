use std::path::Path;

/// Try to load a .env file from current directory, otherwise from parent directory.
/// Simple parser: KEY=VALUE lines, ignores comments and blank lines, preserves existing env vars.
pub fn load_env() {
    let current = Path::new(".env");
    let parent = Path::new("..").join(".env");
    let chosen = if current.exists() {
        Some(current.to_path_buf())
    } else if parent.exists() {
        Some(parent)
    } else {
        None
    };

    if let Some(path) = chosen {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                println!("Loaded env from: {}", path.display());
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some(eq) = line.find('=') {
                        let key = line[..eq].trim();
                        let mut val = line[eq + 1..].trim();
                        // Strip optional surrounding quotes
                        if (val.starts_with('"') && val.ends_with('"'))
                            || (val.starts_with('\'') && val.ends_with('\''))
                        {
                            if val.len() >= 2 {
                                val = &val[1..val.len() - 1];
                            }
                        }
                        // Only set if not already present in environment
                        if std::env::var(key).is_err() {
                            std::env::set_var(key, val);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read .env at {}: {}", path.display(), e);
            }
        }
    } else {
        println!(".env not found in current or parent directory; continuing without loading .env");
    }
}
