use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use dictate_core::local_whisper::{LocalWhisperTranscriber, MODELS};
use dictate_core::{notice, providers, DictateConfig, ErrorKind, LanguageMode, Service, TargetContext, Transcriber};

fn config() -> DictateConfig {
    DictateConfig::parse(r#"{"TranscriptionProvider":"LocalWhisper","CleanupProvider":"None"}"#).unwrap()
}

fn fixture(response: &'static str, delay: Duration) -> (u16, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let read = socket.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        thread::sleep(delay);
        // A timeout test may have closed the connection already.
        let _ = socket.write_all(response.as_bytes());
        request
    });
    (port, handle)
}

#[test]
fn local_models_are_configurable_and_unknown_models_fail_at_startup() {
    for model in MODELS {
        let mut selected = config();
        selected.transcription_model = Some((*model).into());
        assert!(LocalWhisperTranscriber::new(&selected).is_ok(), "{model}");
    }
    assert_eq!(config().effective_transcription_model(), "turbo");
    let mut invalid = config();
    invalid.transcription_model = Some("scribe_v2".into());
    assert!(LocalWhisperTranscriber::new(&invalid).is_err());
    invalid.transcription_model = None;
    invalid.local_whisper_port = 0;
    assert!(LocalWhisperTranscriber::new(&invalid).is_err());
}

#[test]
fn fully_local_mode_reads_no_cloud_keys_even_when_they_are_stored() {
    let (transcriber, cleaner) = providers::build(&config(), |_| panic!("must not read any cloud credential")).unwrap();
    drop(transcriber);
    assert_eq!(
        cleaner.clean("Hallo Welt.", &TargetContext::unknown()).unwrap(),
        "Hallo Welt."
    );
}

#[test]
fn local_requests_carry_wav_model_language_and_unicode_vocabulary() {
    let (port, server) = fixture("HTTP/1.1 200 OK\r\nContent-Length: 40\r\nConnection: close\r\n\r\n{\"text\":\" Hallo Welt. \",\"language\":\"de\"}", Duration::ZERO);
    let mut selected = config();
    selected.local_whisper_port = port;
    selected.transcription_model = Some("small".into());
    selected.vocabulary = vec!["Zürich".into()];
    let wav = dictate_core::wav::from_pcm16(&[1, 0, 2, 0]);
    let result = LocalWhisperTranscriber::new(&selected)
        .unwrap()
        .transcribe(&wav, LanguageMode::German)
        .unwrap();
    assert_eq!(result.text, "Hallo Welt.");
    assert_eq!(result.language_code.as_deref(), Some("de"));
    let request = server.join().unwrap();
    assert!(request.windows(wav.len()).any(|part| part == wav));
    let request = String::from_utf8_lossy(&request);
    assert!(request.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
    for value in [
        "name=\"model\"\r\n\r\nsmall",
        "name=\"language\"\r\n\r\nde",
        "Zürich",
        "verbose_json",
    ] {
        assert!(request.contains(value), "missing {value}");
    }
    assert!(!request.to_lowercase().contains("authorization:"));
    assert!(!request.to_lowercase().contains("xi-api-key:"));
    let auto = LocalWhisperTranscriber::form_fields("turbo", LanguageMode::Auto, &[]);
    assert!(!auto.iter().any(|(name, _)| *name == "language" || *name == "prompt"));
}

#[test]
fn local_redirects_and_model_errors_do_not_fall_back_to_cloud() {
    for response in [
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (port, server) = fixture(response, Duration::ZERO);
        let mut selected = config();
        selected.local_whisper_port = port;
        let error = LocalWhisperTranscriber::new(&selected)
            .unwrap()
            .transcribe(b"wav", LanguageMode::Auto)
            .unwrap_err();
        assert_eq!(error.service(), Some(Service::LocalWhisper));
        assert!(
            error.message().contains("returned"),
            "must return the original status, not follow a redirect"
        );
        server.join().unwrap();
    }
}

#[test]
fn local_timeouts_offer_local_recovery_advice() {
    let (port, server) = fixture("", Duration::from_millis(1500));
    let mut selected = config();
    selected.local_whisper_port = port;
    selected.local_whisper_timeout_seconds = 1;
    let error = LocalWhisperTranscriber::new(&selected)
        .unwrap()
        .transcribe(b"wav", LanguageMode::Auto)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::TimedOut);
    let notice = notice::transcription_failed(&error);
    assert!(notice.detail.contains("LocalWhisperTimeoutSeconds"));
    assert!(!notice.detail.contains("internet"));
    assert!(!notice.detail.contains("credit"));
    server.join().unwrap();
}

#[test]
fn malformed_local_responses_never_copy_transcripts_into_diagnostics() {
    for body in ["private dictated words", r#"{"wrong":"private dictated words"}"#] {
        let error = LocalWhisperTranscriber::parse(body).unwrap_err();
        assert!(!error.message().contains("private dictated words"));
    }
}
