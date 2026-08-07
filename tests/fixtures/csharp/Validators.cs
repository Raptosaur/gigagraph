global using System.Linq;
using System;

namespace Acme.Store;

public static class Validators
{
    public static bool IsValidTitle(string title)
    {
        if (string.IsNullOrWhiteSpace(title))
        {
            return false;
        }
        return title.Trim().Length <= MaxLength();
    }

    public static int MaxLength() => Convert.ToInt32(Math.Pow(2, 7));

    public static Catalog BuildSample()
    {
        var catalog = new Catalog(4);
        catalog.AddBook("Dune", 1965);
        while (catalog.Count() < 2)
        {
            catalog.AddBook("Hyperion", 1989);
        }
        return catalog;
    }

    public static string Classify(int year)
    {
        return year switch
        {
            1900 => "boundary",
            _ => year > 1950 ? "modern" : "classic",
        };
    }

    public static string SafeLabel(Book book)
    {
        return book?.Label() ?? "unknown";
    }
}

public struct Slot
{
    public int Index;

    public int Width()
    {
        return Index * 2;
    }
}

public enum Grade
{
    A,
    B,
}
