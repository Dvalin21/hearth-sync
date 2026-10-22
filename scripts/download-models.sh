#!/usr/bin/env bash
# Download ML models for hearth-sync (optional, for smart search + face detection).
# Models are placed in the data/models/ directory.
set -euo pipefail

MODEL_DIR="${1:-./data/models}"
mkdir -p "$MODEL_DIR"

echo "Downloading CLIP visual model..."
# ViT-B-32 CLIP visual encoder (ONNX)
# Source: https://huggingface.co/immich-app (or OpenAI CLIP converted to ONNX)
echo "NOTE: Model URLs need to be configured for your setup."
echo "Place clip_visual.onnx and face_detection.onnx in $MODEL_DIR"
echo ""
echo "Recommended sources:"
echo "  - CLIP: https://github.com/immich-app/immich (model export scripts)"
echo "  - Face detection: InsightFace buffalo_l (RetinaFace)"
echo ""
echo "For face detection without ONNX models, use the Python helper (ml/inference.py)"
echo "which provides pseudo-detections for testing."