namespace Acme.Store;

public static class Util
{
    public static string Normalize(string name)
    {
        return name.Trim().ToLowerInvariant();
    }
}
