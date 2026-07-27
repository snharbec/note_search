use base64::Engine;
use serde::Deserialize;
use std::error::Error;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";
// `llama3.2-vision` uses Meta's `mllama` architecture, which not every
// Ollama build/backend supports (some error with "unknown model
// architecture: 'mllama'" even after a successful pull). `llava` loads
// almost anywhere but tends to hallucinate/paraphrase rather than transcribe
// verbatim - not reliable for real documents. `minicpm-v` transcribes
// accurately and has broad backend support, so it's the default; override
// with `OLLAMA_VISION_MODEL` if you want to try something else.
const DEFAULT_VISION_MODEL: &str = "minicpm-v";
const RENDER_DPI: &str = "150";

const PROMPT: &str = "Transcribe this document page into clean markdown. \
Use '#'/'##'/'###' headings that match the page's visual hierarchy (font size, \
boldness, position) - do not invent headings that aren't visually distinct. \
Preserve paragraph breaks and render any tables as markdown tables. \
Output only the markdown for this page, no commentary, no page-number label, \
no surrounding code fence.";

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

/// Convert a PDF to markdown by rendering each page to a PNG (via poppler's
/// `pdftoppm`) and asking a local vision-language model served by Ollama to
/// transcribe it. Unlike `lopdf`'s flat text extraction, the model actually
/// sees font size/boldness/columns, so heading and table structure comes
/// through far more reliably on documents with no embedded structure to
/// begin with (e.g. scanned or design-heavy PDFs).
///
/// Requires `pdftoppm` on `PATH` (`brew install poppler` / `apt install
/// poppler-utils`) and a running Ollama instance with the vision model
/// pulled (`ollama pull minicpm-v` by default - see `DEFAULT_VISION_MODEL`).
/// The host defaults to `http://localhost:11434`, overridable via
/// `OLLAMA_HOST`; the model defaults to `minicpm-v`, overridable via
/// `OLLAMA_VISION_MODEL`.
pub fn convert_pdf_vision(path: &Path) -> Result<(String, Option<String>), Box<dyn Error>> {
    let temp_dir = tempfile::tempdir()?;
    let page_prefix = temp_dir.path().join("page");

    let output = Command::new("pdftoppm")
        .arg("-r")
        .arg(RENDER_DPI)
        .arg("-png")
        .arg(path)
        .arg(&page_prefix)
        .output()
        .map_err(|e| format!("failed to run pdftoppm (is poppler installed?): {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "pdftoppm failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let mut page_images: Vec<std::path::PathBuf> = std::fs::read_dir(temp_dir.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
        .collect();
    page_images.sort();

    if page_images.is_empty() {
        return Err("pdftoppm produced no page images".into());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.to_string());
    let url = format!("{}/api/generate", host.trim_end_matches('/'));
    let model =
        std::env::var("OLLAMA_VISION_MODEL").unwrap_or_else(|_| DEFAULT_VISION_MODEL.to_string());

    let mut markdown = String::new();
    let total = page_images.len();
    for (idx, page_path) in page_images.iter().enumerate() {
        println!("Transcribing page {}/{} with {}...", idx + 1, total, model);
        let image_bytes = std::fs::read(page_path)?;
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

        let response = client
            .post(&url)
            .json(&serde_json::json!({
                "model": model,
                "prompt": PROMPT,
                "images": [image_b64],
                "stream": false,
            }))
            .send()?;

        if !response.status().is_success() {
            return Err(format!(
                "Ollama returned status {} for page {}",
                response.status(),
                idx + 1
            )
            .into());
        }

        let parsed: GenerateResponse = response.json()?;
        let page_markdown = strip_code_fence(parsed.response.trim());
        if !page_markdown.is_empty() {
            if !markdown.is_empty() {
                markdown.push_str("\n\n");
            }
            markdown.push_str(&page_markdown);
        }
    }

    let title = markdown
        .lines()
        .find_map(|line| line.strip_prefix('#').map(|rest| rest.trim_start_matches('#').trim()))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    Ok((markdown, title))
}

/// Vision models don't reliably obey "no surrounding code fence" - strip one
/// if the whole response is wrapped in a single ``` block. Leaving it in
/// would make `extract_segments`'s fence-tracking treat the entire page as
/// "inside a fence" and skip heading detection for all of it.
fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() >= 2
        && lines[0].trim_start().starts_with("```")
        && lines[lines.len() - 1].trim().starts_with("```")
    {
        return lines[1..lines.len() - 1].join("\n");
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_fence_removes_markdown_fence() {
        let input = "```markdown\n# Title\n\nBody text.\n```";
        assert_eq!(strip_code_fence(input), "# Title\n\nBody text.");
    }

    #[test]
    fn test_strip_code_fence_removes_bare_fence() {
        let input = "```\n# Title\n```";
        assert_eq!(strip_code_fence(input), "# Title");
    }

    #[test]
    fn test_strip_code_fence_leaves_unfenced_text_alone() {
        let input = "# Title\n\nBody text.";
        assert_eq!(strip_code_fence(input), "# Title\n\nBody text.");
    }

    #[test]
    fn test_strip_code_fence_leaves_inline_fence_alone() {
        // A real fenced code block inside otherwise-unwrapped output should
        // not be touched - only a fence wrapping the *entire* response.
        let input = "# Title\n\n```bash\necho hi\n```\n\nMore text.";
        assert_eq!(strip_code_fence(input), input);
    }
}
