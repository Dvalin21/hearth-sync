use std::path::PathBuf;
use std::process::Command;

/// Inference engine that delegates to a Python helper script.
/// This avoids requiring the ort crate (which needs Rust 1.92+).
/// The Python script uses torch for ML inference.
pub struct InferenceEngine {
    pub model_dir: PathBuf,
    pub python_path: String,
    pub script_path: PathBuf,
}

impl InferenceEngine {
    pub fn new(model_dir: PathBuf) -> Self {
        Self {
            model_dir,
            python_path: "python3".to_string(),
            script_path: PathBuf::from("ml/inference.py"),
        }
    }

    /// Check if ML dependencies are available.
    pub fn is_available(&self) -> bool {
        Command::new(&self.python_path)
            .arg("-c")
            .arg("import torch; print('ok')")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Encode an image to a CLIP embedding via Python helper.
    pub fn encode_image(&self, image_path: &str) -> Result<Vec<f32>, String> {
        let output = Command::new(&self.python_path)
            .arg(&self.script_path)
            .arg("encode_image")
            .arg(image_path)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        // Parse JSON output: {"embedding": [0.1, 0.2, ...]}
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| e.to_string())?;
        let arr = json["embedding"].as_array().ok_or("no embedding in response")?;
        Ok(arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
    }

    /// Encode text to a CLIP embedding via Python helper.
    pub fn encode_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let output = Command::new(&self.python_path)
            .arg(&self.script_path)
            .arg("encode_text")
            .arg(text)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| e.to_string())?;
        let arr = json["embedding"].as_array().ok_or("no embedding in response")?;
        Ok(arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
    }

    /// Detect faces in an image via Python helper.
    pub fn detect_faces(&self, image_path: &str) -> Result<Vec<FaceDetection>, String> {
        let output = Command::new(&self.python_path)
            .arg(&self.script_path)
            .arg("detect_faces")
            .arg(image_path)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| e.to_string())?;
        let faces = json["faces"].as_array().cloned().unwrap_or_default();
        Ok(faces
            .iter()
            .map(|f| FaceDetection {
                x: f["x"].as_f64().unwrap_or(0.0) as f32,
                y: f["y"].as_f64().unwrap_or(0.0) as f32,
                w: f["w"].as_f64().unwrap_or(0.0) as f32,
                h: f["h"].as_f64().unwrap_or(0.0) as f32,
                confidence: f["confidence"].as_f64().unwrap_or(0.0) as f32,
                embedding: f["embedding"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
                    .unwrap_or_default(),
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct FaceDetection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub confidence: f32,
    pub embedding: Vec<f32>,
}
