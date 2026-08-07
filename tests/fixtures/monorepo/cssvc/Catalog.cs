using System;

namespace Acme.Store;

public class Catalog
{
    public string Register(string rawName)
    {
        var name = Util.Normalize(rawName);
        Console.WriteLine(name);
        return name;
    }
}
