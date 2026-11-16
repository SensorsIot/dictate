"""Prepare local Dictate configuration after validating the model runtime."""
import argparse
import json
from pathlib import Path

from server import MODELS, load_windows_cuda_runtime


def update_config(config, model, port, hotkey):
    result = dict(config)
    values = {
        'TranscriptionProvider': 'LocalWhisper', 'TranscriptionModel': model,
        'CleanupProvider': 'None', 'Language': 'Auto', 'LocalWhisperPort': port,
    }
    if hotkey != 'Keep':
        values.update(HotkeyVirtualKey={'CapsLock': 20, 'RightWindows': 92, 'RightCtrl': 163}[hotkey],
                      SuppressHotkey=True, ShortPressPassesThrough=hotkey == 'CapsLock')
    # Match Dictate's case-insensitive configuration, without duplicate keys.
    for key, value in values.items():
        for existing in list(result):
            if existing.lower() == key.lower():
                del result[existing]
        result[key] = value
    return result


def main():
    import json5
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--model', choices=MODELS, required=True)
    parser.add_argument('--device', choices=('cpu', 'cuda'), required=True)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--hotkey', choices=('Keep', 'CapsLock', 'RightWindows', 'RightCtrl'), default='Keep')
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error('port must be between 1 and 65535')
    if args.model.endswith('.en'):
        parser.error('Choose a multilingual model for automatic German/English detection.')
    # Parse before downloading or changing anything. Preserve device/hotkey and
    # unrelated settings; the PowerShell installer saves the original file.
    config = json5.loads(args.config.read_text(encoding='utf-8-sig')) if args.config.exists() else {}
    if not isinstance(config, dict):
        raise ValueError('Dictate configuration must be an object.')
    handles = load_windows_cuda_runtime()
    from faster_whisper import WhisperModel
    compute_type = 'int8_float16' if args.device == 'cuda' else 'int8'
    print(f'Downloading/loading {args.model} on {args.device} ({compute_type})...', flush=True)
    engine = WhisperModel(args.model, device=args.device, compute_type=compute_type)
    print('Model loaded successfully.', flush=True)
    updated = update_config(config, args.model, args.port, args.hotkey)
    args.config.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.config.with_suffix(args.config.suffix + '.installing')
    temporary.write_text(json.dumps(updated, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    temporary.replace(args.config)


if __name__ == '__main__':
    main()
