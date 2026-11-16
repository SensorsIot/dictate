"""Resident local speech-to-text service. Audio and transcripts stay in memory."""

import argparse
from email import policy
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import logging
import os
from pathlib import Path
import sys
import sysconfig
import threading


MODELS = (
    "tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium",
    "medium.en", "large-v1", "large-v2", "large-v3", "turbo",
)
MAX_REQUEST_BYTES = 8 * 1024 * 1024


def load_windows_cuda_runtime():
    """Find optional NVIDIA pip DLLs without changing the machine's PATH."""
    if sys.platform != "win32":
        return []
    root = Path(sysconfig.get_path("purelib")) / "nvidia"
    directories = sorted({dll.parent for dll in root.rglob("*.dll")})
    # CTranslate2 and its dependent DLLs can use different Windows loaders.
    # Support both DLL-directory lookup and process-local PATH lookup. Keep
    # the returned handles alive for the lifetime of the inference engine.
    handles = [os.add_dll_directory(str(directory)) for directory in directories]
    if directories:
        os.environ["PATH"] = os.pathsep.join([*(str(directory) for directory in directories), os.environ.get("PATH", "")])
    return handles


def default_config_path():
    if sys.platform == "win32":
        return Path(os.environ["APPDATA"]) / "dictate" / "config.json"
    if sys.platform == "darwin":
        return Path.home() / "Library/Application Support/dictate/config.json"
    return Path.home() / ".config/dictate/config.json"


def settings(entries):
    if not isinstance(entries, dict):
        raise ValueError("Expected a config object.")
    entries = {key.lower(): value for key, value in entries.items()}
    if str(entries.get("transcriptionprovider", "ElevenLabs")).lower() != "localwhisper":
        raise ValueError("Set TranscriptionProvider to LocalWhisper in config.json first.")
    model = entries.get("transcriptionmodel")
    if model is None or (isinstance(model, str) and not model.strip()):
        model = "turbo"
    if not isinstance(model, str) or model.strip() not in MODELS:
        raise ValueError("TranscriptionModel must be one of: " + ", ".join(MODELS))
    port = entries.get("localwhisperport", 8178)
    if isinstance(port, bool) or not isinstance(port, (int, float)) or int(port) != port or not 1 <= port <= 65535:
        raise ValueError("LocalWhisperPort must be an integer between 1 and 65535.")
    return model.strip(), int(port)


def parse_form(content_type, body):
    if not content_type.lower().startswith("multipart/form-data;"):
        raise ValueError("Expected multipart/form-data.")
    message = BytesParser(policy=policy.default).parsebytes(
        b"Content-Type: " + content_type.encode("ascii") + b"\r\nMIME-Version: 1.0\r\n\r\n" + body
    )
    if not message.is_multipart() or message.defects:
        raise ValueError("Invalid multipart request.")
    fields = {}
    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        if name in fields:
            raise ValueError("Duplicate form field.")
        payload = part.get_payload(decode=True)
        if name and payload is not None:
            fields[name] = payload if name == "file" else payload.decode("utf-8")
    if not fields.get("file"):
        raise ValueError("Missing audio file.")
    if fields.get("language") not in (None, "de", "en"):
        raise ValueError("Language must be de, en, or omitted for automatic detection.")
    if fields.get("response_format") != "verbose_json":
        raise ValueError("Expected verbose_json response format.")
    return fields


def make_server(model_name, engine, port):
    inference_lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(30)

        def log_message(self, *_args):
            # Access logs can contain request data. Only startup metadata is printed.
            pass

        def reply(self, status, value):
            body = json.dumps(value, ensure_ascii=False).encode("utf-8")
            try:
                self.send_response(status)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError, TimeoutError):
                pass  # The caller may have reached its transcription timeout.

        def do_GET(self):
            if self.path == "/health":
                self.reply(200, {"model": model_name, "status": "ready"})
            else:
                self.reply(404, {"error": "Unknown path."})

        def do_POST(self):
            if self.path != "/v1/audio/transcriptions":
                self.reply(404, {"error": "Unknown path."})
                return
            if self.headers.get("Origin"):
                self.reply(403, {"error": "Browser requests are not supported."})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= MAX_REQUEST_BYTES:
                    self.reply(413, {"error": "Audio request must be at most 8 MiB."})
                    return
                body = self.rfile.read(length)
                if len(body) != length:
                    raise ValueError("Incomplete request.")
                fields = parse_form(self.headers.get("Content-Type", ""), body)
            except (ValueError, UnicodeError, TimeoutError):
                self.reply(400, {"error": "Invalid audio request."})
                return
            if fields.get("model") != model_name:
                self.reply(400, {"error": "Model mismatch. Restart the local service and dictate after editing config.json."})
                return
            if not inference_lock.acquire(blocking=False):
                self.reply(503, {"error": "Local Whisper is still processing. Wait, then dictate again."})
                return
            try:
                segments, info = engine.transcribe(
                    io.BytesIO(fields["file"]),
                    language=fields.get("language"),
                    task="transcribe",
                    initial_prompt=fields.get("prompt"),
                    beam_size=5,
                    vad_filter=True,
                    condition_on_previous_text=False,
                )
                # faster-whisper performs recognition while iterating the generator.
                text = "".join(segment.text for segment in segments).strip()
                self.reply(200, {"text": text, "language": info.language})
            except Exception:
                # Engine error messages may carry input data. Keep content out of logs.
                print("Local transcription failed. Check GPU memory, CUDA libraries and the audio input.", file=sys.stderr)
                self.reply(500, {"error": "Local transcription failed. Check GPU memory and runtime installation."})
            finally:
                inference_lock.release()

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    return server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, default=default_config_path())
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--compute-type", choices=("int8", "int8_float16", "float16", "float32"))
    parser.add_argument("--offline", action="store_true", help="Use cached model files only; no downloads.")
    args = parser.parse_args()
    try:
        _cuda_dll_handles = load_windows_cuda_runtime()
        import json5
        import ctranslate2
        from faster_whisper import WhisperModel

        model, port = settings(json5.loads(args.config.read_text(encoding="utf-8-sig")))
        device = args.device
        if device == "auto":
            device = "cuda" if ctranslate2.get_cuda_device_count() > 0 else "cpu"
        compute_type = args.compute_type or ("int8_float16" if device == "cuda" else "int8")
        logging.getLogger("faster_whisper").setLevel(logging.ERROR)
        print(f"Loading Whisper {model} on {device} ({compute_type})...", flush=True)
        engine = WhisperModel(model, device=device, compute_type=compute_type, local_files_only=args.offline)
        with make_server(model, engine, port) as server:
            print(f"Ready: http://127.0.0.1:{port} — model {model}. Keep this process running.", flush=True)
            server.serve_forever()
    except KeyboardInterrupt:
        return 0
    except Exception as error:
        print(f"Local Whisper could not start: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
