#[cfg(feature = "ml")]
use ort::session::Session;
#[cfg(feature = "ml")]
use ort::value::Tensor;

use std::path::PathBuf;

/// Inference engine. Uses ort (ONNX Runtime) when compiled with `ml` feature.
/// Falls back to Python/torch helper otherwise.
pub struct InferenceEngine {
    pub model_dir: PathBuf,
    #[cfg(feature = "ml")]
    pub clip_session: Option<std::sync::Mutex<Session>>,
    #[cfg(feature = "ml")]
    pub face_session: Option<std::sync::Mutex<Session>>,
}

impl InferenceEngine {
    pub fn new(model_dir: PathBuf) -> Self {
        #[cfg(feature = "ml")]
        {
            let clip_path = model_dir.join("clip_visual.onnx");
            let face_path = model_dir.join("face_detection.onnx");
            let env = ort::init().with_name("hearth-sync").build().ok();
            let clip_session = if clip_path.exists() {
                env.as_ref().and_then(|e| Session::builder(e).ok()?.commit_from_file(&clip_path).ok().map(std::sync::Mutex::new))
            } else {
                None
            };
            let face_session = if face_path.exists() {
                env.as_ref().and_then(|e| Session::builder(e).ok()?.commit_from_file(&face_path).ok().map(std::sync::Mutex::new))
            } else {
                None
            };
            Self { model_dir, clip_session, face_session }
        }
        #[cfg(not(feature = "ml"))]
        {
            Self { model_dir }
        }
    }

    /// Check if ML is available (models loaded or Python helper present).
    pub fn is_available(&self) -> bool {
        #[cfg(feature = "ml")]
        {
            self.clip_session.is_some() || self.face_session.is_some()
        }
        #[cfg(not(feature = "ml"))]
        {
            std::process::Command::new("python3")
                .arg("-c")
                .arg("import torch; print('ok')")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }

    /// Encode an image to a CLIP embedding.
    pub fn encode_image(&self, image_path: &str) -> Result<Vec<f32>, String> {
        #[cfg(feature = "ml")]
        {
            if let Some(session) = &self.clip_session {
                // Load image, preprocess, run inference
                // (Simplified: in production, use proper preprocessing pipeline)
                let img = image::open(image_path).map_err(|e| e.to_string())?;
                let resized = img.resize_exact(224, 224, image::imageops::FilterType::Triangle);
                let rgb = resized.to_rgb8();
                // Normalize and convert to tensor (CHW format)
                let mut input_data = vec![0.0f32; 3 * 224 * 224];
                for y in 0..224 {
                    for x in 0..224 {
                        let pixel = rgb.get_pixel(x, y);
                        input_data[0 * 224 * 224 + (y as usize) * 224 + (x as usize)] = (pixel[0] as f32 / 255.0 - 0.48145466) / 0.26862954;
                        input_data[1 * 224 * 224 + (y as usize) * 224 + (x as usize)] = (pixel[1] as f32 / 255.0 - 0.4578275) / 0.26130258;
                        input_data[2 * 224 * 224 + (y as usize) * 224 + (x as usize)] = (pixel[2] as f32 / 255.0 - 0.40821073) / 0.27577711;
                    }
                }
                let tensor = Tensor::from_array(([1, 3, 224, 224], input_data)).map_err(|e| e.to_string())?;
                let mut session = session.lock().unwrap();
                let outputs = session.run(ort::inputs![tensor]).map_err(|e| e.to_string())?;
                let embedding = outputs[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
                Ok(embedding.1.to_vec())
            } else {
                // Fall back to Python helper
                self.python_encode_image(image_path)
            }
        }
        #[cfg(not(feature = "ml"))]
        {
            self.python_encode_image(image_path)
        }
    }

    /// Encode text to a CLIP embedding.
    pub fn encode_text(&self, text: &str) -> Result<Vec<f32>, String> {
        #[cfg(feature = "ml")]
        {
            if self.clip_session.is_some() {
                // Text encoding requires tokenizer; fall back to Python for now
                self.python_encode_text(text)
            } else {
                self.python_encode_text(text)
            }
        }
        #[cfg(not(feature = "ml"))]
        {
            self.python_encode_text(text)
        }
    }

    /// Detect faces in an image.
    pub fn detect_faces(&self, image_path: &str) -> Result<Vec<FaceDetection>, String> {
        #[cfg(feature = "ml")]
        {
            if let Some(session) = &self.face_session {
                // Run face detection model
                // (Simplified: in production, use proper preprocessing)
                let img = image::open(image_path).map_err(|e| e.to_string())?;
                let resized = img.resize_exact(640, 640, image::imageops::FilterType::Triangle);
                let rgb = resized.to_rgb8();
                let mut input_data = vec![0.0f32; 3 * 640 * 640];
                for y in 0..640 {
                    for x in 0..640 {
                        let pixel = rgb.get_pixel(x, y);
                        input_data[0 * 640 * 640 + (y as usize) * 640 + (x as usize)] = pixel[0] as f32 / 255.0;
                        input_data[1 * 640 * 640 + (y as usize) * 640 + (x as usize)] = pixel[1] as f32 / 255.0;
                        input_data[2 * 640 * 640 + (y as usize) * 640 + (x as usize)] = pixel[2] as f32 / 255.0;
                    }
                }
                let tensor = Tensor::from_array(([1, 3, 640, 640], input_data)).map_err(|e| e.to_string())?;
                let mut session = session.lock().unwrap();
                let outputs = session.run(ort::inputs![tensor]).map_err(|e| e.to_string())?;
                // Parse outputs (format depends on model; this is simplified)
                let _ = outputs;
                // Return empty for now - proper parsing requires model-specific output format
                Ok(vec![])
            } else {
                self.python_detect_faces(image_path)
            }
        }
        #[cfg(not(feature = "ml"))]
        {
            self.python_detect_faces(image_path)
        }
    }

    // --- Python helper fallbacks ---

    fn python_encode_image(&self, image_path: &str) -> Result<Vec<f32>, String> {
        let output = std::process::Command::new("python3")
            .arg(self.model_dir.join("../ml/inference.py"))
            .arg("encode_image")
            .arg(image_path)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        let arr = json["embedding"].as_array().ok_or("no embedding")?;
        Ok(arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
    }

    fn python_encode_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let output = std::process::Command::new("python3")
            .arg(self.model_dir.join("../ml/inference.py"))
            .arg("encode_text")
            .arg(text)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        let arr = json["embedding"].as_array().ok_or("no embedding")?;
        Ok(arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
    }

    fn python_detect_faces(&self, image_path: &str) -> Result<Vec<FaceDetection>, String> {
        let output = std::process::Command::new("python3")
            .arg(self.model_dir.join("../ml/inference.py"))
            .arg("detect_faces")
            .arg(image_path)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
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

    pub fn ocr(&self, image_path: &str) -> Result<String, String> {
        let out = std::process::Command::new("python3")
            .arg(self.model_dir.join("../ml/inference.py"))
            .arg("ocr")
            .arg(image_path)
            .arg(&self.model_dir)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        if let Some(err) = v.get("error") { return Err(err.as_str().unwrap_or("ocr failed").to_string()); }
        Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string())
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
