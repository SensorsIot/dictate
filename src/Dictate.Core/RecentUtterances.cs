namespace Dictate.Core;

/// <summary>
/// The last few delivered utterances, newest first, held in memory only
/// (FR-17.2). Nothing here reaches disk and nothing survives the process.
///
/// Lives in Core rather than beside the tray menu so the trimming rule is
/// covered by the host tier. The Windows side owns only the rendering.
/// </summary>
public sealed class RecentUtterances
{
    private readonly LinkedList<Utterance> _items = new();

    public RecentUtterances(int capacity)
    {
        if (capacity < 1)
        {
            throw new ArgumentOutOfRangeException(nameof(capacity), capacity, "Capacity must be at least 1.");
        }

        Capacity = capacity;
    }

    public int Capacity { get; }

    public int Count => _items.Count;

    /// <summary>Newest first.</summary>
    public IReadOnlyList<Utterance> Items => _items.ToArray();

    /// <summary>Adds an utterance, dropping the oldest once capacity is exceeded.</summary>
    public void Add(Utterance utterance)
    {
        _items.AddFirst(utterance);

        while (_items.Count > Capacity)
        {
            _items.RemoveLast();
        }
    }

    public void Clear() => _items.Clear();
}
