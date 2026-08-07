using System;

namespace Acme.Store;

// Declared BEFORE CrateStore so name-based same-file resolution would pick
// this Stamp (smaller fn id); only the two-hop receiver rung
// (load.Store.Stamp -> load: Crate -> Crate.Store: CrateStore) picks the
// right one. The integration test relies on this ordering.
public class LabelPrinter
{
    private int _count;

    public void Stamp(string label)
    {
        _count += 1;
        Console.WriteLine(label);
    }
}

public class CrateStore
{
    private int _total;

    public void Stamp(string label)
    {
        _total += 1;
    }
}

public class Crate
{
    public CrateStore Store;
}

public class Dockyard
{
    public void Seal(Crate load)
    {
        load.Store.Stamp("sealed");
    }
}
