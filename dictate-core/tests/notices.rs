//! FR-7.4, FR-7.5, FR-14.6: every notice tells the user what happened to
//! their words and what to do, in words, never in status codes or JSON.

use dictate_core::notice::{self, DETAIL_LIMIT, TITLE_LIMIT};
use dictate_core::{Error, ErrorKind, Notice, Service};

/// Verbatim from the diagnostic log, 2026-10-27 08:56.
const ELEVENLABS_BUSY: &str = r#"{"detail":{"type":"rate_limit_error","code":"system_busy","message":"We are sorry, the system is experiencing heavy traffic, please try again.","status":"system_busy","request_id":"667533d20c37c3d577ac185c55a42c7c","docs_url":"https://elevenlabs.io/docs"}}"#;

/// Verbatim from the diagnostic log, 2026-10-22 11:08.
const ELEVENLABS_QUOTA: &str = r#"{"detail":{"type":"invalid_request","code":"quota_exceeded","message":"This request exceeds your quota of 10000. You have 0 credits remaining, while 13 credits are required for this request.","status":"quota_exceeded","request_id":"48b32bc808d1"}}"#;

const ELEVENLABS_BAD_KEY: &str = r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#;
const ELEVENLABS_CONCURRENCY: &str = r#"{"detail":{"status":"too_many_concurrent_requests","message":"Too many concurrent requests for your plan."}}"#;
const ANTHROPIC_NO_CREDIT: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits."}}"#;
const ANTHROPIC_OVERLOADED: &str = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
const ANTHROPIC_BAD_KEY: &str = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
const ANTHROPIC_INTERNAL: &str = r#"{"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#;
const INFOMANIAK_V1_UNAUTHORISED: &str =
    r#"{"result":"error","error":{"code":"not_authorized","description":"Authorization required"}}"#;
const BAD_MODEL: &str = r#"{"detail":{"status":"invalid_model","message":"model_id must be one of scribe_v1, scribe_v2"}}"#;

fn kind(service: Service, status: u16, body: &str) -> ErrorKind {
    Error::from_response(service, status, body).kind()
}

#[test]
fn each_failure_is_sorted_by_what_the_user_can_do() {
    assert_eq!(kind(Service::ElevenLabs, 429, ELEVENLABS_BUSY), ErrorKind::Overloaded);
    assert_eq!(kind(Service::ElevenLabs, 401, ELEVENLABS_QUOTA), ErrorKind::OutOfCredit);
    assert_eq!(kind(Service::ElevenLabs, 401, ELEVENLABS_BAD_KEY), ErrorKind::KeyRejected);
    assert_eq!(kind(Service::ElevenLabs, 429, ELEVENLABS_CONCURRENCY), ErrorKind::RateLimited);
    assert_eq!(kind(Service::ElevenLabs, 422, BAD_MODEL), ErrorKind::Rejected);
    assert_eq!(kind(Service::Anthropic, 400, ANTHROPIC_NO_CREDIT), ErrorKind::OutOfCredit);
    assert_eq!(kind(Service::Anthropic, 529, ANTHROPIC_OVERLOADED), ErrorKind::Overloaded);
    assert_eq!(kind(Service::Anthropic, 401, ANTHROPIC_BAD_KEY), ErrorKind::KeyRejected);
    assert_eq!(kind(Service::Anthropic, 500, ANTHROPIC_INTERNAL), ErrorKind::ServerFault);
    assert_eq!(kind(Service::Infomaniak, 401, INFOMANIAK_V1_UNAUTHORISED), ErrorKind::KeyRejected);
    assert_eq!(kind(Service::Infomaniak, 402, "{}"), ErrorKind::OutOfCredit);
    assert_eq!(kind(Service::Infomaniak, 502, "<html><body>Bad Gateway</body></html>"), ErrorKind::ServerFault);
}

#[test]
fn the_service_sentence_is_found_in_every_provider_shape() {
    let said = |service, status, body| Error::from_response(service, status, body).said().map(str::to_owned);

    assert_eq!(
        said(Service::ElevenLabs, 429, ELEVENLABS_BUSY).as_deref(),
        Some("We are sorry, the system is experiencing heavy traffic, please try again.")
    );
    assert_eq!(said(Service::Anthropic, 401, ANTHROPIC_BAD_KEY).as_deref(), Some("invalid x-api-key"));
    assert_eq!(said(Service::Infomaniak, 401, INFOMANIAK_V1_UNAUTHORISED).as_deref(), Some("Authorization required"));
    assert_eq!(said(Service::Infomaniak, 502, "<html>Bad Gateway</html>"), None);
}

#[test]
fn the_log_keeps_the_whole_response() {
    let error = Error::from_response(Service::ElevenLabs, 429, ELEVENLABS_BUSY);

    assert!(error.message().starts_with("ElevenLabs returned 429 Too Many Requests: "));
    assert!(error.message().contains("667533d20c37c3d577ac185c55a42c7c"));
}

#[test]
fn an_outage_says_wait_and_that_it_is_not_the_users_fault() {
    let notice = notice::transcription_failed(&Error::from_response(Service::ElevenLabs, 429, ELEVENLABS_BUSY));

    assert_eq!(notice.title, "ElevenLabs is overloaded");
    assert!(notice.detail.starts_with("Your words were not transcribed."));
    assert!(notice.detail.contains("not on your side"));
    assert!(notice.detail.contains("Wait a few minutes"));
}

#[test]
fn an_empty_elevenlabs_account_says_where_to_top_up() {
    let notice = notice::transcription_failed(&Error::from_response(Service::ElevenLabs, 401, ELEVENLABS_QUOTA));

    assert_eq!(notice.title, "ElevenLabs credit used up");
    assert!(notice.detail.contains("elevenlabs.io"));
    assert!(!notice.detail.contains("Re-enter"), "a quota problem must not send the user to change a working key");
}

#[test]
fn an_empty_anthropic_account_still_types_and_says_where_to_top_up() {
    let notice = notice::cleanup_failed(&Error::from_response(Service::Anthropic, 400, ANTHROPIC_NO_CREDIT));

    assert_eq!(notice.title, "Anthropic credit used up");
    assert!(notice.detail.starts_with("Your words were typed as spoken"));
    assert!(notice.detail.contains("platform.claude.com"));
}

#[test]
fn an_overloaded_cleanup_asks_nothing_of_the_user() {
    let notice = notice::cleanup_failed(&Error::from_response(Service::Anthropic, 529, ANTHROPIC_OVERLOADED));

    assert_eq!(notice.title, "Anthropic is overloaded");
    assert!(notice.detail.contains("Nothing to do"));
}

#[test]
fn a_rejected_key_points_to_the_menu_item_that_replaces_it() {
    let notice = notice::transcription_failed(&Error::from_response(Service::ElevenLabs, 401, ELEVENLABS_BAD_KEY));

    assert_eq!(notice.title, "ElevenLabs rejected the API key");
    assert!(notice.detail.contains("Re-enter API keys…"));
}

#[test]
fn a_rejected_model_names_the_setting_and_quotes_the_service() {
    let notice = notice::transcription_failed(&Error::from_response(Service::ElevenLabs, 422, BAD_MODEL));

    assert!(notice.detail.contains("TranscriptionModel"));
    assert!(notice.detail.contains("scribe_v1, scribe_v2"));
}

#[test]
fn a_transcription_deadline_names_the_service() {
    let error = Error::of(Service::Infomaniak, ErrorKind::TimedOut, "Infomaniak did not finish transcribing within 30 s");

    assert_eq!(notice::transcription_failed(&error).title, "Infomaniak is not answering");
}

#[test]
fn an_unreachable_service_says_check_the_connection() {
    // Port 9 (discard) is closed on any CI runner: a refused connection, no network needed.
    let refused = reqwest::blocking::Client::new().get("http://127.0.0.1:9/").send().unwrap_err();
    let error = Error::transport(Service::ElevenLabs, refused);

    assert_eq!(error.kind(), ErrorKind::Unreachable);
    let notice = notice::transcription_failed(&error);
    assert_eq!(notice.title, "Cannot reach ElevenLabs");
    assert!(notice.detail.contains("internet connection"));
}

#[test]
fn the_platforms_log_detail_is_not_shown() {
    let notice = notice::typing_failed(
        "dictate could not type into this window. Windows blocks typing into a window that runs as administrator. \
         [SendInput 0/84, error 5]",
        "Ctrl+V",
    );

    assert!(!notice.detail.contains("SendInput"));
    assert!(notice.detail.ends_with("Press Ctrl+V to paste it."));
}

/// The negative: whatever a service sends back, the user never sees a status
/// code, JSON or a request id, and the notice fits a Windows balloon.
#[test]
fn no_notice_shows_codes_or_json_or_overflows() {
    let long = format!(r#"{{"error":{{"message":"{}"}}}}"#, "very long explanation ".repeat(40));
    let bodies = [
        ELEVENLABS_BUSY,
        ELEVENLABS_QUOTA,
        ELEVENLABS_BAD_KEY,
        ELEVENLABS_CONCURRENCY,
        ANTHROPIC_NO_CREDIT,
        ANTHROPIC_OVERLOADED,
        INFOMANIAK_V1_UNAUTHORISED,
        BAD_MODEL,
        "<html><body>502 Bad Gateway</body></html>",
        "",
        long.as_str(),
    ];
    let statuses = [400, 401, 402, 403, 404, 422, 429, 500, 502, 503, 529];
    let services = [Service::ElevenLabs, Service::Anthropic, Service::Infomaniak];

    let mut notices: Vec<Notice> = Vec::new();
    for service in services {
        for status in statuses {
            for body in bodies {
                let error = Error::from_response(service, status, body);
                notices.push(notice::transcription_failed(&error));
                notices.push(notice::cleanup_failed(&error));
            }
        }
    }
    notices.push(notice::transcription_failed(&Error::new("Scribe response was not JSON: <html>")));

    for notice in notices {
        let shown = format!("{} {}", notice.title, notice.detail);
        assert!(notice.title.chars().count() <= TITLE_LIMIT, "title too long: {}", notice.title);
        assert!(notice.detail.chars().count() <= DETAIL_LIMIT, "detail too long: {}", notice.detail);
        assert!(!shown.contains('{') && !shown.contains('<'), "markup shown: {shown}");
        assert!(!shown.contains("request_id") && !shown.contains("returned"), "internals shown: {shown}");
        for status in statuses {
            assert!(!shown.contains(&status.to_string()), "status code shown: {shown}");
        }
    }
}
