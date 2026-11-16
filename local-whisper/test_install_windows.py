import unittest

from configure_windows import update_config


class InstallerConfigurationTests(unittest.TestCase):
    def test_existing_audio_hotkey_and_private_settings_survive(self):
        original = {'InputDevice': 'Headset', 'HotkeyVirtualKey': 163,
                    'SuppressHotkey': True, 'Vocabulary': ['ESP32'],
                    'EnableDiagnosticLog': False, 'Language': 'German'}
        updated = update_config(original, 'small', 8178, 'Keep')
        self.assertEqual(updated['InputDevice'], original['InputDevice'])
        self.assertEqual(updated['HotkeyVirtualKey'], 163)
        self.assertEqual(updated['Vocabulary'], ['ESP32'])
        self.assertFalse(updated['EnableDiagnosticLog'])
        self.assertEqual(updated['Language'], 'Auto')
        self.assertEqual(original['Language'], 'German')

    def test_case_insensitive_keys_cannot_leave_cloud_settings_active(self):
        updated = update_config({'transcriptionprovider': 'ElevenLabs',
                                 'cleanupProvider': 'Anthropic', 'LANGUAGE': 'German'},
                                'turbo', 8180, 'Keep')
        normalized = {key.lower(): value for key, value in updated.items()}
        self.assertEqual(len(normalized), len(updated))
        self.assertEqual(normalized['transcriptionprovider'], 'LocalWhisper')
        self.assertEqual(normalized['cleanupprovider'], 'None')
        self.assertEqual(normalized['language'], 'Auto')
        self.assertEqual(normalized['localwhisperport'], 8180)

    def test_caps_lock_profile_retains_a_tap_and_right_windows_suppresses_start(self):
        caps = update_config({}, 'small', 8178, 'CapsLock')
        self.assertEqual(caps['HotkeyVirtualKey'], 20)
        self.assertTrue(caps['SuppressHotkey'])
        self.assertTrue(caps['ShortPressPassesThrough'])
        win = update_config(caps, 'turbo', 8178, 'RightWindows')
        self.assertEqual(win['HotkeyVirtualKey'], 92)
        self.assertTrue(win['SuppressHotkey'])
        self.assertFalse(win['ShortPressPassesThrough'])


if __name__ == '__main__':
    unittest.main()
