import http.client
import json
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

from server import MODELS, make_server, settings


def multipart(**fields):
    body = bytearray()
    for name, value in fields.items():
        body.extend(f'--test-boundary\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n'.encode())
        body.extend(value if isinstance(value, bytes) else value.encode())
        body.extend(b"\r\n")
    body.extend(b"--test-boundary--\r\n")
    return bytes(body)


class ConfigurationTests(unittest.TestCase):
    def test_models_are_selected_from_the_same_case_insensitive_config(self):
        for model in MODELS:
            self.assertEqual(settings({"transcriptionprovider": "localwhisper", "TRANSCRIPTIONMODEL": model}), (model, 8178))
        for default in (None, "", "  "):
            self.assertEqual(settings({"TranscriptionProvider": "LocalWhisper", "TranscriptionModel": default}), ("turbo", 8178))

    def test_invalid_settings_fail_before_loading_a_model(self):
        for values in ({}, {"TranscriptionModel": "invalid"}, {"LocalWhisperPort": 0}, {"LocalWhisperPort": True}, {"LocalWhisperPort": 1.5}):
            config = {"TranscriptionProvider": "LocalWhisper", **values}
            if not values:
                config["TranscriptionProvider"] = "ElevenLabs"
            with self.assertRaises(ValueError):
                settings(config)


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.engine = Mock()
        self.engine.transcribe.return_value = (iter([SimpleNamespace(text=" Hallo Zürich. ")]), SimpleNamespace(language="de"))
        self.server = make_server("turbo", self.engine, 0)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def post(self, fields, extra_headers=None):
        connection = http.client.HTTPConnection("127.0.0.1", self.server.server_address[1], timeout=3)
        headers = {"Content-Type": "multipart/form-data; boundary=test-boundary", **(extra_headers or {})}
        connection.request("POST", "/v1/audio/transcriptions", multipart(**fields), headers)
        response = connection.getresponse()
        status, body = response.status, json.loads(response.read())
        connection.close()
        return status, body

    def test_audio_stays_in_memory_and_transcription_never_translates(self):
        audio = b"RIFF\x00WAVE\xff\x80"
        status, body = self.post(dict(file=audio, model="turbo", response_format="verbose_json", language="de", prompt="Zürich, ESP32"))
        self.assertEqual(status, 200)
        self.assertEqual(body, {"text": "Hallo Zürich.", "language": "de"})
        args, kwargs = self.engine.transcribe.call_args
        self.assertEqual(args[0].getvalue(), audio)
        self.assertEqual(kwargs["task"], "transcribe")
        self.assertEqual(kwargs["language"], "de")
        self.assertEqual(kwargs["initial_prompt"], "Zürich, ESP32")

    def test_automatic_language_detection_is_not_forced_to_english(self):
        self.assertEqual(self.post(dict(file=b"wav", model="turbo", response_format="verbose_json"))[0], 200)
        self.assertIsNone(self.engine.transcribe.call_args.kwargs["language"])

    def test_model_mismatch_invalid_forms_and_browser_requests_never_run_inference(self):
        valid = dict(file=b"wav", model="turbo", response_format="verbose_json")
        for changes in ({"model": "small"}, {"language": "invalid"}, {"file": b""}):
            self.assertEqual(self.post({**valid, **changes})[0], 400)
        self.assertEqual(self.post(valid, {"Origin": "https://example.com"})[0], 403)
        self.engine.transcribe.assert_not_called()

    def test_inference_failures_are_errors_and_hide_input_data(self):
        self.engine.transcribe.side_effect = RuntimeError("private dictated words")
        status, body = self.post(dict(file=b"wav", model="turbo", response_format="verbose_json"))
        self.assertEqual(status, 500)
        self.assertNotIn("private dictated words", json.dumps(body))


if __name__ == "__main__":
    unittest.main()
