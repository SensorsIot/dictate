using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class RecentUtterancesTests
{
    private static Utterance Text(string text) =>
        new() { Status = UtteranceStatus.Ok, Text = text };

    [Fact]
    public void Newest_comes_first()
    {
        var recent = new RecentUtterances(5);

        recent.Add(Text("one"));
        recent.Add(Text("two"));

        Assert.Equal(["two", "one"], recent.Items.Select(u => u.Text));
    }

    [Fact]
    public void The_oldest_is_dropped_once_capacity_is_exceeded()
    {
        var recent = new RecentUtterances(3);

        foreach (var word in new[] { "a", "b", "c", "d", "e" })
        {
            recent.Add(Text(word));
        }

        Assert.Equal(3, recent.Count);
        Assert.Equal(["e", "d", "c"], recent.Items.Select(u => u.Text));
    }

    [Fact]
    public void Capacity_of_one_keeps_only_the_latest()
    {
        var recent = new RecentUtterances(1);

        recent.Add(Text("first"));
        recent.Add(Text("second"));

        Assert.Equal(["second"], recent.Items.Select(u => u.Text));
    }

    [Fact]
    public void Items_is_a_snapshot_that_adding_does_not_disturb()
    {
        // The tray menu enumerates this while rebuilding itself. If Items were a
        // live view, adding during that walk would throw — which is exactly the
        // crash this class was extracted after.
        var recent = new RecentUtterances(5);
        recent.Add(Text("one"));

        var snapshot = recent.Items;
        recent.Add(Text("two"));

        Assert.Single(snapshot);
    }

    [Fact]
    public void Clear_empties_it()
    {
        var recent = new RecentUtterances(5);
        recent.Add(Text("one"));

        recent.Clear();

        Assert.Equal(0, recent.Count);
        Assert.Empty(recent.Items);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(-1)]
    public void A_capacity_below_one_is_rejected(int capacity)
    {
        Assert.Throws<ArgumentOutOfRangeException>(() => new RecentUtterances(capacity));
    }
}
