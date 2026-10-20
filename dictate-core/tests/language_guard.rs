use std::sync::Mutex;

use dictate_core::{Error, GermanOrEnglish, LanguageMode, ScribeTranscriber, Transcriber, Transcript};

/// Answers by requested language, and records what it was asked for.
struct Fake {
    detected: &'static str,
    german: f64,
    english: f64,
    asked: Mutex<Vec<LanguageMode>>,
}

impl Fake {
    fn new(detected: &'static str, german: f64, english: f64) -> Self {
        Self { detected, german, english, asked: Mutex::new(Vec::new()) }
    }
}

impl Transcriber for Fake {
    fn transcribe(&self, _wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        self.asked.lock().unwrap().push(language);
        Ok(match language {
            LanguageMode::Auto => Transcript::new("hallo", Some(self.detected)),
            LanguageMode::German => Transcript::new("hallo (de)", Some("deu")).with_confidence(Some(self.german)),
            LanguageMode::English => Transcript::new("hallo (en)", Some("eng")).with_confidence(Some(self.english)),
        })
    }
}

fn asked(guard: &GermanOrEnglish, fake: &Fake) -> (Transcript, Vec<LanguageMode>) {
    let transcript = guard.transcribe(b"", LanguageMode::Auto).unwrap();
    let mut asked = fake.asked.lock().unwrap().clone();
    asked.sort_by_key(|l| l.name());
    (transcript, asked)
}

// H-24 ------------------------------------------------------------------------

#[test]
fn german_or_english_detected_is_kept_after_one_request() {
    for detected in ["deu", "german", "en", "ENGLISH"] {
        let fake = std::sync::Arc::new(Fake::new(detected, -0.1, -0.1));
        let guard = GermanOrEnglish::new(Box::new(ArcFake(fake.clone())));
        let (transcript, asked) = asked(&guard, &fake);
        assert_eq!(asked, [LanguageMode::Auto], "{detected}: no second request");
        assert!(matches!(transcript.language_code.as_deref(), Some("de" | "en")), "normalised: {detected}");
    }
}

#[test]
fn a_third_language_is_redone_as_german_and_english_and_the_surer_wins() {
    // FR-10.8. Must not: Dutch (or anything else) delivered.
    let fake = std::sync::Arc::new(Fake::new("nl", -0.5, -0.1));
    let guard = GermanOrEnglish::new(Box::new(ArcFake(fake.clone())));
    let (transcript, asked) = asked(&guard, &fake);
    assert_eq!(asked, [LanguageMode::Auto, LanguageMode::English, LanguageMode::German]);
    assert_eq!(transcript.text, "hallo (en)");
    assert_eq!(transcript.language_code.as_deref(), Some("en"));

    let fake = std::sync::Arc::new(Fake::new("gsw", -0.05, -0.9));
    let guard = GermanOrEnglish::new(Box::new(ArcFake(fake.clone())));
    let (transcript, _) = asked(&guard, &fake);
    assert_eq!(transcript.text, "hallo (de)", "Swiss German becomes Standard German");
}

#[test]
fn a_pinned_language_is_passed_straight_through() {
    let fake = std::sync::Arc::new(Fake::new("nl", -0.5, -0.1));
    let guard = GermanOrEnglish::new(Box::new(ArcFake(fake.clone())));
    let transcript = guard.transcribe(b"", LanguageMode::German).unwrap();
    assert_eq!(*fake.asked.lock().unwrap(), [LanguageMode::German]);
    assert_eq!(transcript.language_code.as_deref(), Some("de"));
}

#[test]
fn picking_prefers_a_score_then_german_then_whatever_succeeded() {
    let de = || Ok(Transcript::new("de", Some("de")));
    let en = || Ok(Transcript::new("en", Some("en")));
    assert_eq!(GermanOrEnglish::pick(de(), en()).unwrap().text, "de", "no scores: German");
    assert_eq!(
        GermanOrEnglish::pick(de(), Ok(Transcript::new("en", None).with_confidence(Some(-1.0)))).unwrap().text,
        "en",
        "any score beats none"
    );
    assert_eq!(GermanOrEnglish::pick(Err(Error::new("x")), en()).unwrap().text, "en");
    assert!(GermanOrEnglish::pick(Err(Error::new("x")), Err(Error::new("y"))).is_err());
}

#[test]
fn reported_languages_are_recognised_in_every_form() {
    for code in ["de", "deu", "ger", "German", " german "] {
        assert_eq!(LanguageMode::from_reported(code), Some(LanguageMode::German), "{code}");
    }
    for code in ["en", "eng", "English"] {
        assert_eq!(LanguageMode::from_reported(code), Some(LanguageMode::English), "{code}");
    }
    for code in ["gsw", "nl", "french", ""] {
        assert_eq!(LanguageMode::from_reported(code), None, "{code}");
    }
}

#[test]
fn scribe_confidence_is_the_mean_word_logprob() {
    let transcript = ScribeTranscriber::parse(
        r#"{"language_code":"deu","text":"Guten Morgen","words":[
            {"text":"Guten","type":"word","logprob":-0.1},
            {"text":" ","type":"spacing","logprob":-5.0},
            {"text":"Morgen","type":"word","logprob":-0.3}]}"#,
    )
    .unwrap();
    assert!((transcript.confidence.unwrap() + 0.2).abs() < 1e-9, "spacing is not scored");
    assert_eq!(ScribeTranscriber::parse(r#"{"text":"x"}"#).unwrap().confidence, None);
}

/// Lets a test keep a handle on the fake after boxing it.
struct ArcFake(std::sync::Arc<Fake>);

impl Transcriber for ArcFake {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        self.0.transcribe(wav, language)
    }
}
