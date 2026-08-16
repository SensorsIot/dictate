using System.Net.Http.Headers;
using System.Text.Json;

namespace Dictate.Core;

public interface ITranscriber
{
    Task<Transcript> TranscribeAsync(byte[] wav, LanguageMode language, CancellationToken ct);
}

/// <summary>
/// ElevenLabs Scribe, batch endpoint. One POST per utterance: we only know the
/// audio is complete when the key comes up, so there is nothing to stream.
/// </summary>
public sealed class ScribeTranscriber : ITranscriber
{
    public const string Endpoint = "https://api.elevenlabs.io/v1/speech-to-text";

    private readonly HttpClient _http;
    private readonly string _apiKey;
    private readonly string _modelId;

    public ScribeTranscriber(HttpClient http, string apiKey, string modelId)
    {
        _http = http;
        _apiKey = apiKey;
        _modelId = modelId;
    }

    public async Task<Transcript> TranscribeAsync(byte[] wav, LanguageMode language, CancellationToken ct)
    {
        using var form = new MultipartFormDataContent();

        var audio = new ByteArrayContent(wav);
        audio.Headers.ContentType = new MediaTypeHeaderValue("audio/wav");
        form.Add(audio, "file", "utterance.wav");

        form.Add(new StringContent(_modelId), "model_id");

        // Off deliberately: "(laughter)" and speaker labels are noise when the
        // output is going straight into a text field.
        form.Add(new StringContent("false"), "tag_audio_events");
        form.Add(new StringContent("false"), "diarize");

        if (language.ToLanguageCode() is { } code)
        {
            form.Add(new StringContent(code), "language_code");
        }

        using var request = new HttpRequestMessage(HttpMethod.Post, Endpoint) { Content = form };
        request.Headers.Add("xi-api-key", _apiKey);

        using var response = await _http.SendAsync(request, HttpCompletionOption.ResponseContentRead, ct);
        var body = await response.Content.ReadAsStringAsync(ct);

        if (!response.IsSuccessStatusCode)
        {
            // The body carries the useful part — an invalid model_id comes back
            // as a 422 listing the values it will accept.
            throw new TranscriptionException(
                $"Scribe returned {(int)response.StatusCode} {response.ReasonPhrase}: {Truncate(body, 500)}");
        }

        return Parse(body);
    }

    internal static Transcript Parse(string json)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;

        if (!root.TryGetProperty("text", out var textElement))
        {
            throw new TranscriptionException($"Scribe response had no 'text' field: {Truncate(json, 300)}");
        }

        var languageCode = root.TryGetProperty("language_code", out var lang) && lang.ValueKind == JsonValueKind.String
            ? lang.GetString()
            : null;

        return new Transcript(textElement.GetString()?.Trim() ?? "", languageCode);
    }

    private static string Truncate(string value, int max) =>
        value.Length <= max ? value : value[..max] + "…";
}

public sealed class TranscriptionException : Exception
{
    public TranscriptionException(string message) : base(message) { }
}
