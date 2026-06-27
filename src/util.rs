use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub fn scan_supported_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    scan_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn scan_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".mdlive") {
                continue;
            }
            scan_recursive(&path, files)?;
        } else if path.is_file() && is_supported_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

/// Return every subdirectory under `base_dir` as a base-relative path string,
/// so empty directories still show up in the sidebar tree.
pub(crate) fn scan_directories(base_dir: &Path) -> Vec<String> {
    let mut dirs = Vec::new();
    scan_dirs_recursive(base_dir, base_dir, &mut dirs);
    dirs.sort();
    dirs
}

fn scan_dirs_recursive(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|n| n == ".mdlive") {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().to_string();
            if !rel.is_empty() {
                out.push(rel);
            }
        }
        scan_dirs_recursive(root, &path, out);
    }
}

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false)
}

pub(crate) fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("txt"))
        .unwrap_or(false)
}

pub(crate) fn is_json_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
}

pub(crate) fn is_csv_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("csv"))
        .unwrap_or(false)
}

pub(crate) fn is_yaml_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
        .unwrap_or(false)
}

pub(crate) fn is_toml_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("toml"))
        .unwrap_or(false)
}

pub(crate) fn is_mermaid_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("mmd"))
        .unwrap_or(false)
}

pub(crate) fn is_supported_file(path: &Path) -> bool {
    is_markdown_file(path)
        || is_text_file(path)
        || is_json_file(path)
        || is_csv_file(path)
        || is_yaml_file(path)
        || is_toml_file(path)
        || is_mermaid_file(path)
}

pub(crate) fn file_type_class(filename: &str) -> &'static str {
    let path = Path::new(filename);
    if is_markdown_file(path) {
        "markdown"
    } else if is_json_file(path) {
        "json"
    } else if is_csv_file(path) {
        "csv"
    } else if is_yaml_file(path) {
        "yaml"
    } else if is_toml_file(path) {
        "toml"
    } else if is_mermaid_file(path) {
        "mermaid"
    } else if is_text_file(path) {
        "plaintext"
    } else {
        "unknown"
    }
}

pub(crate) fn is_image_file(file_path: &str) -> bool {
    let extension = Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    matches!(
        extension.to_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico"
    )
}

pub(crate) fn guess_image_content_type(file_path: &str) -> String {
    let extension = Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    match extension.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_is_markdown_file() {
        assert!(is_markdown_file(Path::new("test.md")));
        assert!(is_markdown_file(Path::new("/path/to/file.md")));
        assert!(is_markdown_file(Path::new("test.markdown")));
        assert!(is_markdown_file(Path::new("/path/to/file.markdown")));
        assert!(is_markdown_file(Path::new("test.MD")));
        assert!(is_markdown_file(Path::new("test.Md")));
        assert!(is_markdown_file(Path::new("test.MARKDOWN")));
        assert!(is_markdown_file(Path::new("test.MarkDown")));

        assert!(!is_markdown_file(Path::new("test.txt")));
        assert!(!is_markdown_file(Path::new("test.rs")));
        assert!(!is_markdown_file(Path::new("test.html")));
        assert!(!is_markdown_file(Path::new("test")));
        assert!(!is_markdown_file(Path::new("README")));
    }

    #[test]
    fn test_is_image_file() {
        assert!(is_image_file("test.png"));
        assert!(is_image_file("test.jpg"));
        assert!(is_image_file("test.jpeg"));
        assert!(is_image_file("test.gif"));
        assert!(is_image_file("test.svg"));
        assert!(is_image_file("test.webp"));
        assert!(is_image_file("test.bmp"));
        assert!(is_image_file("test.ico"));

        assert!(is_image_file("test.PNG"));
        assert!(is_image_file("test.JPG"));
        assert!(is_image_file("test.JPEG"));

        assert!(is_image_file("/path/to/image.png"));
        assert!(is_image_file("./images/photo.jpg"));

        assert!(!is_image_file("test.txt"));
        assert!(!is_image_file("test.md"));
        assert!(!is_image_file("test.rs"));
        assert!(!is_image_file("test"));
    }

    #[test]
    fn test_guess_image_content_type() {
        assert_eq!(guess_image_content_type("test.png"), "image/png");
        assert_eq!(guess_image_content_type("test.jpg"), "image/jpeg");
        assert_eq!(guess_image_content_type("test.jpeg"), "image/jpeg");
        assert_eq!(guess_image_content_type("test.gif"), "image/gif");
        assert_eq!(guess_image_content_type("test.svg"), "image/svg+xml");
        assert_eq!(guess_image_content_type("test.webp"), "image/webp");
        assert_eq!(guess_image_content_type("test.bmp"), "image/bmp");
        assert_eq!(guess_image_content_type("test.ico"), "image/x-icon");

        assert_eq!(guess_image_content_type("test.PNG"), "image/png");
        assert_eq!(guess_image_content_type("test.JPG"), "image/jpeg");

        assert_eq!(
            guess_image_content_type("test.xyz"),
            "application/octet-stream"
        );
        assert_eq!(guess_image_content_type("test"), "application/octet-stream");
    }

    #[test]
    fn test_is_text_file() {
        assert!(is_text_file(Path::new("test.txt")));
        assert!(is_text_file(Path::new("/path/to/file.txt")));
        assert!(is_text_file(Path::new("test.TXT")));
        assert!(is_text_file(Path::new("test.Txt")));
        assert!(!is_text_file(Path::new("test.md")));
        assert!(!is_text_file(Path::new("test.json")));
        assert!(!is_text_file(Path::new("test")));
    }

    #[test]
    fn test_is_json_file() {
        assert!(is_json_file(Path::new("test.json")));
        assert!(is_json_file(Path::new("/path/to/file.json")));
        assert!(is_json_file(Path::new("test.JSON")));
        assert!(is_json_file(Path::new("test.Json")));
        assert!(!is_json_file(Path::new("test.md")));
        assert!(!is_json_file(Path::new("test.txt")));
        assert!(!is_json_file(Path::new("test")));
    }

    #[test]
    fn test_is_csv_file() {
        assert!(is_csv_file(Path::new("test.csv")));
        assert!(is_csv_file(Path::new("/path/to/file.csv")));
        assert!(is_csv_file(Path::new("test.CSV")));
        assert!(is_csv_file(Path::new("test.Csv")));
        assert!(!is_csv_file(Path::new("test.tsv")));
        assert!(!is_csv_file(Path::new("test.md")));
        assert!(!is_csv_file(Path::new("test")));
    }

    #[test]
    fn test_is_yaml_file() {
        assert!(is_yaml_file(Path::new("test.yaml")));
        assert!(is_yaml_file(Path::new("test.yml")));
        assert!(is_yaml_file(Path::new("test.YAML")));
        assert!(is_yaml_file(Path::new("test.Yml")));
        assert!(is_yaml_file(Path::new("/path/to/file.yaml")));
        assert!(!is_yaml_file(Path::new("test.yamll")));
        assert!(!is_yaml_file(Path::new("test.json")));
        assert!(!is_yaml_file(Path::new("test")));
    }

    #[test]
    fn test_is_toml_file() {
        assert!(is_toml_file(Path::new("test.toml")));
        assert!(is_toml_file(Path::new("/path/to/Cargo.toml")));
        assert!(is_toml_file(Path::new("test.TOML")));
        assert!(is_toml_file(Path::new("test.Toml")));
        assert!(!is_toml_file(Path::new("test.tml")));
        assert!(!is_toml_file(Path::new("test.ini")));
        assert!(!is_toml_file(Path::new("test")));
    }

    #[test]
    fn test_is_mermaid_file() {
        assert!(is_mermaid_file(Path::new("test.mmd")));
        assert!(is_mermaid_file(Path::new("/path/to/diagram.mmd")));
        assert!(is_mermaid_file(Path::new("test.MMD")));
        assert!(is_mermaid_file(Path::new("test.Mmd")));
        assert!(!is_mermaid_file(Path::new("test.mermaid")));
        assert!(!is_mermaid_file(Path::new("test.md")));
        assert!(!is_mermaid_file(Path::new("test")));
    }

    #[test]
    fn test_is_supported_file() {
        assert!(is_supported_file(Path::new("test.md")));
        assert!(is_supported_file(Path::new("test.markdown")));
        assert!(is_supported_file(Path::new("test.txt")));
        assert!(is_supported_file(Path::new("test.json")));
        assert!(is_supported_file(Path::new("test.JSON")));
        assert!(is_supported_file(Path::new("test.csv")));
        assert!(is_supported_file(Path::new("test.yaml")));
        assert!(is_supported_file(Path::new("test.yml")));
        assert!(is_supported_file(Path::new("test.toml")));
        assert!(!is_supported_file(Path::new("test.rs")));
        assert!(!is_supported_file(Path::new("test.html")));
        assert!(!is_supported_file(Path::new("test")));
    }

    #[test]
    fn test_file_type_class() {
        assert_eq!(file_type_class("test.md"), "markdown");
        assert_eq!(file_type_class("test.markdown"), "markdown");
        assert_eq!(file_type_class("test.json"), "json");
        assert_eq!(file_type_class("test.JSON"), "json");
        assert_eq!(file_type_class("test.txt"), "plaintext");
        assert_eq!(file_type_class("test.TXT"), "plaintext");
        assert_eq!(file_type_class("test.csv"), "csv");
        assert_eq!(file_type_class("test.CSV"), "csv");
        assert_eq!(file_type_class("test.yaml"), "yaml");
        assert_eq!(file_type_class("test.yml"), "yaml");
        assert_eq!(file_type_class("test.toml"), "toml");
        assert_eq!(file_type_class("test.TOML"), "toml");
        assert_eq!(file_type_class("test.mmd"), "mermaid");
        assert_eq!(file_type_class("test.MMD"), "mermaid");
        assert_eq!(file_type_class("test.rs"), "unknown");
        assert_eq!(file_type_class("test"), "unknown");
    }

    #[test]
    fn test_scan_supported_files_empty_directory() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let result = scan_supported_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_scan_supported_files_finds_all_types() {
        let temp_dir = tempdir().expect("Failed to create temp dir");

        fs::write(temp_dir.path().join("test1.md"), "# Test 1").expect("Failed to write");
        fs::write(temp_dir.path().join("test2.markdown"), "# Test 2").expect("Failed to write");
        fs::write(temp_dir.path().join("test3.txt"), "text").expect("Failed to write");
        fs::write(temp_dir.path().join("test4.json"), "{}").expect("Failed to write");
        fs::write(temp_dir.path().join("test5.csv"), "a,b,c").expect("Failed to write");
        fs::write(temp_dir.path().join("test6.yaml"), "a: 1").expect("Failed to write");
        fs::write(temp_dir.path().join("test7.yml"), "b: 2").expect("Failed to write");
        fs::write(temp_dir.path().join("test8.toml"), "k = 1").expect("Failed to write");
        fs::write(temp_dir.path().join("README"), "readme").expect("Failed to write");
        fs::write(temp_dir.path().join("test.rs"), "fn main(){}").expect("Failed to write");

        let result = scan_supported_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 8);

        let filenames: Vec<_> = result
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(filenames.contains(&"test1.md"));
        assert!(filenames.contains(&"test2.markdown"));
        assert!(filenames.contains(&"test3.txt"));
        assert!(filenames.contains(&"test4.json"));
        assert!(filenames.contains(&"test5.csv"));
        assert!(filenames.contains(&"test6.yaml"));
        assert!(filenames.contains(&"test7.yml"));
        assert!(filenames.contains(&"test8.toml"));
    }

    #[test]
    fn test_scan_supported_files_includes_subdirectories() {
        let temp_dir = tempdir().expect("Failed to create temp dir");

        fs::write(temp_dir.path().join("root.md"), "# Root").expect("Failed to write");
        let sub_dir = temp_dir.path().join("subdir");
        fs::create_dir(&sub_dir).expect("Failed to create subdir");
        fs::write(sub_dir.join("nested.json"), "{}").expect("Failed to write");

        let result = scan_supported_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 2);

        let filenames: Vec<_> = result
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(filenames.contains(&"root.md"));
        assert!(filenames.contains(&"nested.json"));
    }

    #[test]
    fn test_scan_supported_files_case_insensitive() {
        let temp_dir = tempdir().expect("Failed to create temp dir");

        fs::write(temp_dir.path().join("test1.md"), "# Test 1").expect("Failed to write");
        fs::write(temp_dir.path().join("test2.MD"), "# Test 2").expect("Failed to write");
        fs::write(temp_dir.path().join("test3.TXT"), "text").expect("Failed to write");
        fs::write(temp_dir.path().join("test4.JSON"), "{}").expect("Failed to write");

        let result = scan_supported_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_scan_supported_files_deep_nesting() {
        let temp_dir = tempdir().expect("Failed to create temp dir");

        fs::write(temp_dir.path().join("root.md"), "# Root").expect("Failed to write");
        let level1 = temp_dir.path().join("level1");
        fs::create_dir(&level1).expect("Failed to create dir");
        fs::write(level1.join("level1.txt"), "text").expect("Failed to write");
        let level2 = level1.join("level2");
        fs::create_dir(&level2).expect("Failed to create dir");
        fs::write(level2.join("level2.json"), "{}").expect("Failed to write");

        let result = scan_supported_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 3);
    }
}
