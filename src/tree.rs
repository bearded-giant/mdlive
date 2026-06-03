use minijinja::value::Value;
use std::collections::{BTreeMap, HashMap};

use crate::state::FileInfo;

pub(crate) fn build_file_tree(file_infos: &[FileInfo], dir_paths: &[String]) -> Vec<Value> {
    let ts_map: HashMap<&str, (u64, u64)> = file_infos
        .iter()
        .map(|fi| (fi.name.as_str(), (fi.modified, fi.created)))
        .collect();
    let names: Vec<String> = file_infos.iter().map(|fi| fi.name.clone()).collect();
    build_tree_level(&names, dir_paths, "", &ts_map)
}

fn build_tree_level(
    paths: &[String],
    dir_paths: &[String],
    prefix: &str,
    ts_map: &HashMap<&str, (u64, u64)>,
) -> Vec<Value> {
    // (sub_file_paths, sub_dir_paths) per immediate directory at this level
    let mut dirs: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    let mut files: Vec<String> = Vec::new();

    for path in paths {
        if let Some(slash_pos) = path.find('/') {
            let dir_name = &path[..slash_pos];
            let rest = &path[slash_pos + 1..];
            dirs.entry(dir_name.to_string())
                .or_default()
                .0
                .push(rest.to_string());
        } else {
            files.push(path.clone());
        }
    }

    for path in dir_paths {
        if path.is_empty() {
            continue;
        }
        if let Some(slash_pos) = path.find('/') {
            let dir_name = &path[..slash_pos];
            let rest = &path[slash_pos + 1..];
            dirs.entry(dir_name.to_string())
                .or_default()
                .1
                .push(rest.to_string());
        } else {
            // an empty directory at this level -- ensure it has a tree entry
            dirs.entry(path.clone()).or_default();
        }
    }

    let mut items: Vec<(String, Value)> = Vec::new();

    for (dir_name, (sub_paths, sub_dirs)) in &dirs {
        let dir_prefix = if prefix.is_empty() {
            dir_name.clone()
        } else {
            format!("{}/{}", prefix, dir_name)
        };
        let children = build_tree_level(sub_paths, sub_dirs, &dir_prefix, ts_map);

        // aggregate: modified = max of children, created = min of children
        let (dir_modified, dir_created) = aggregate_timestamps(&children);

        let mut map = HashMap::new();
        map.insert("name".to_string(), Value::from(dir_name.clone()));
        map.insert("is_dir".to_string(), Value::from(true));
        map.insert("dir_path".to_string(), Value::from(dir_prefix.clone()));
        map.insert("children".to_string(), Value::from(children));
        map.insert("modified".to_string(), Value::from(dir_modified));
        map.insert("created".to_string(), Value::from(dir_created));
        items.push((dir_name.to_lowercase(), Value::from_object(map)));
    }

    for file_name in &files {
        let full_path = if prefix.is_empty() {
            file_name.clone()
        } else {
            format!("{}/{}", prefix, file_name)
        };
        let (modified, created) = ts_map.get(full_path.as_str()).copied().unwrap_or((0, 0));
        let file_type = crate::util::file_type_class(file_name);
        let mut map = HashMap::new();
        map.insert("name".to_string(), Value::from(file_name.clone()));
        map.insert("path".to_string(), Value::from(full_path));
        map.insert("is_dir".to_string(), Value::from(false));
        map.insert("modified".to_string(), Value::from(modified));
        map.insert("created".to_string(), Value::from(created));
        map.insert("file_type".to_string(), Value::from(file_type));
        items.push((file_name.to_lowercase(), Value::from_object(map)));
    }

    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.into_iter().map(|(_, v)| v).collect()
}

fn aggregate_timestamps(children: &[Value]) -> (u64, u64) {
    let mut max_modified: u64 = 0;
    let mut min_created: u64 = u64::MAX;

    for child in children {
        if let Ok(m) = child.get_attr("modified") {
            if let Some(v) = m.as_str().and_then(|s| s.parse::<u64>().ok()) {
                max_modified = max_modified.max(v);
                min_created = min_created.min(
                    child
                        .get_attr("created")
                        .ok()
                        .and_then(|c| c.as_str().and_then(|s| s.parse::<u64>().ok()))
                        .unwrap_or(u64::MAX),
                );
            } else if let Ok(v) = u64::try_from(m.clone()) {
                max_modified = max_modified.max(v);
                min_created = min_created.min(
                    child
                        .get_attr("created")
                        .ok()
                        .and_then(|c| u64::try_from(c).ok())
                        .unwrap_or(u64::MAX),
                );
            }
        }
    }

    if min_created == u64::MAX {
        min_created = 0;
    }
    (max_modified, min_created)
}
