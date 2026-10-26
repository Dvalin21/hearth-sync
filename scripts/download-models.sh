#!/usr/bin/env bash
# Download ML models for hearth-sync (optional, for smart search + face detection).
set -euo pipefail

MODEL_DIR="${1:-./data/models}"
mkdir -p "$MODEL_DIR"

BASE="https://huggingface.co/Marqo/onnx-open_clip-ViT-B-32/resolve/main"

echo "Downloading CLIP visual model (ONNX, ~168MB)..."
curl -L -o "$MODEL_DIR/clip_visual.onnx" \
  "$BASE/onnx16-open_clip-ViT-B-32-laion2b_e16-visual.onnx"

echo "Downloading CLIP textual model (ONNX, ~122MB)..."
curl -L -o "$MODEL_DIR/clip_textual.onnx" \
  "$BASE/onnx16-open_clip-ViT-B-32-laion2b_e16-textual.onnx"

echo "Downloading face detection model (RetinaFace, ~113MB)..."
curl -L -o "$MODEL_DIR/face_detection.onnx" \
  "https://huggingface.co/vidyamdeveloper/retinaface-onnx/resolve/main/retinaface_standard_conversion.onnx"

echo ""
echo "Models downloaded to $MODEL_DIR"
echo "  - clip_visual.onnx: image encoding (Rust ort path)"
echo "  - clip_textual.onnx: text encoding (reference; Rust uses Python/transformers)"
echo "  - face_detection.onnx: face detection (post-processing TBD)"
echo ""
echo "Text encoding and face detection fall back to the Python helper (ml/inference.py)"
echo "which uses transformers (CLIP) and a center-face heuristic respectively."
