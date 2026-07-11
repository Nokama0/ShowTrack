use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Only `w<digits>/<file>.<ext>` or `original/<file>.<ext>`; file is a single
/// path segment of [A-Za-z0-9._-] with an extension. Blocks traversal.
pub fn valid_rel(rel: &str) -> bool {
    let Some((size, file)) = rel.split_once('/') else { return false; };
    let size_ok = size == "original"
        || (size.len() > 1 && size.starts_with('w') && size[1..].chars().all(|c| c.is_ascii_digit()));
    let file_ok = !file.is_empty()
        && file.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !file.contains("..")
        && file.rsplit_once('.').map(|(stem, ext)| !stem.is_empty() && !ext.is_empty()).unwrap_or(false);
    size_ok && file_ok
}

fn content_type(rel: &str) -> &'static str {
    match rel.rsplit_once('.').map(|(_, e)| e) {
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Write via unique temp file + rename so concurrent writers can't interleave.
async fn write_atomic(file: &Path, bytes: &[u8]) -> Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = file.with_extension(format!("tmp-{}-{n}", std::process::id()));
    tokio::fs::write(&tmp, bytes).await?;
    match tokio::fs::rename(&tmp, file).await {
        Ok(()) => Ok(()),
        // a concurrent writer won the rename race (Windows rename fails onto an
        // existing file) — their bytes are complete, ours are equivalent
        Err(_) if tokio::fs::try_exists(file).await.unwrap_or(false) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Ok(())
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e.into())
        }
    }
}

/// Return the image bytes, downloading into `root/<rel>` on first use.
pub async fn fetch_cached(root: &Path, http: &reqwest::Client, rel: &str) -> Result<Vec<u8>> {
    if !valid_rel(rel) {
        return Err(AppError::msg(format!("bad image path {rel:?}")));
    }
    let file: PathBuf = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Ok(bytes) = tokio::fs::read(&file).await {
        return Ok(bytes);
    }
    let url = format!("https://image.tmdb.org/t/p/{rel}");
    let resp = http.get(&url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?.to_vec();
    if let Some(dir) = file.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    write_atomic(&file, &bytes).await?;
    Ok(bytes)
}

/// poster:// protocol handler body (registered in lib.rs).
pub async fn serve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    use tauri::Manager;
    let rel = request.uri().path().trim_start_matches('/').to_string();
    let root = app.path().app_data_dir().map(|d| d.join("images"));
    let result = match root {
        Ok(root) => {
            let http = app.state::<crate::AppState>().http.clone();
            fetch_cached(&root, &http, &rel).await
        }
        Err(e) => Err(AppError::Msg(e.to_string())),
    };
    let resp = match result {
        Ok(bytes) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", content_type(&rel))
            .header("Cache-Control", "max-age=31536000")
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)
            .unwrap(),
        Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
    };
    responder.respond(resp);
}

/// Total bytes under the cache dir (for the Settings screen).
pub fn dir_size(root: &Path) -> u64 {
    fn walk(dir: &Path, acc: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, acc);
                } else if let Ok(md) = e.metadata() {
                    *acc += md.len();
                }
            }
        }
    }
    let mut acc = 0;
    walk(root, &mut acc);
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rel_paths() {
        assert!(valid_rel("w342/abc.jpg"));
        assert!(valid_rel("original/x-1_2.png"));
        assert!(!valid_rel("w342/../secret.txt"));
        assert!(!valid_rel("../etc/passwd"));
        assert!(!valid_rel("w342/a/b.jpg"));
        assert!(!valid_rel("c:/windows/x.jpg"));
        assert!(!valid_rel("w342/noext"));
        assert!(!valid_rel(""));
    }

    #[tokio::test]
    async fn write_atomic_survives_concurrent_writers() {
        let dir = std::env::temp_dir().join(format!("showtrack-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("img.jpg");
        let a = vec![0xAAu8; 64 * 1024];
        let b = vec![0xBBu8; 64 * 1024];
        let (ra, rb) = tokio::join!(write_atomic(&target, &a), write_atomic(&target, &b));
        ra.unwrap();
        rb.unwrap();
        let got = tokio::fs::read(&target).await.unwrap();
        // whole-file integrity: the result is exactly one writer's payload, no interleaving
        assert!(got == a || got == b);
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
