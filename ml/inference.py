#!/usr/bin/env python3
"""Companion ML inference helper for hearth-sync.
Called as: python3 inference.py <command> <image_path_or_text> <model_dir>
Commands: encode_image, encode_text, detect_faces
Outputs JSON to stdout.
"""
import sys, json, os

def encode_image(path, model_dir):
    try:
        import torch
        from PIL import Image
        # Placeholder: return a dummy embedding (real impl would load CLIP model)
        # In production, load CLIP from model_dir and encode
        img = Image.open(path)
        w, h = img.size
        # Deterministic pseudo-embedding based on image dimensions
        import hashlib
        seed = int(hashlib.md5(path.encode()).hexdigest(), 16) % (2**32)
        torch.manual_seed(seed)
        emb = torch.randn(512).tolist()
        print(json.dumps({"embedding": emb}))
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stderr)
        sys.exit(1)

def encode_text(text, model_dir):
    try:
        import torch
        import hashlib
        seed = int(hashlib.md5(text.encode()).hexdigest(), 16) % (2**32)
        torch.manual_seed(seed)
        emb = torch.randn(512).tolist()
        print(json.dumps({"embedding": emb}))
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stderr)
        sys.exit(1)

def detect_faces(path, model_dir):
    try:
        import torch
        from PIL import Image
        img = Image.open(path)
        w, h = img.size
        # Placeholder: return a single face covering center
        faces = [{
            "x": 0.25, "y": 0.15, "w": 0.5, "h": 0.6,
            "confidence": 0.95,
            "embedding": [0.0] * 128,
        }]
        print(json.dumps({"faces": faces, "imageWidth": w, "imageHeight": h}))
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stderr)
        sys.exit(1)

def ocr(path, model_dir):
    try:
        # Prefer pytesseract if available
        try:
            import pytesseract
            from PIL import Image
            img = Image.open(path)
            text = pytesseract.image_to_string(img)
            print(json.dumps({"text": text}))
            return
        except Exception:
            pass
        # Fallback: tesseract CLI
        import subprocess
        out = subprocess.run(["tesseract", path, "stdout"], capture_output=True, text=True)
        if out.returncode == 0:
            print(json.dumps({"text": out.stdout}))
            return
        print(json.dumps({"error": "OCR not available: install tesseract or pytesseract"}), file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    cmd = sys.argv[1]
    arg = sys.argv[2]
    model_dir = sys.argv[3] if len(sys.argv) > 3 else ""
    if cmd == "encode_image":
        encode_image(arg, model_dir)
    elif cmd == "encode_text":
        encode_text(arg, model_dir)
    elif cmd == "detect_faces":
        detect_faces(arg, model_dir)
    elif cmd == "ocr":
        ocr(arg, model_dir)
    else:
        print(json.dumps({"error": f"unknown command {cmd}"}), file=sys.stderr)
        sys.exit(1)
EOF
chmod +x /home/keith/host/hearth-sync/ml/inference.py
echo done