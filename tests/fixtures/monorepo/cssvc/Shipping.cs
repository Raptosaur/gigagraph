using System;
using System.Collections.Generic;

namespace Acme.Store;

public interface IShipStore
{
    void Persist(string id);
}

// Declared BEFORE DbShipStore so that, without the container binding, bare
// hierarchy expansion would rank this implementor first (smaller fn id) —
// the integration test would then fail, proving AddScoped is load-bearing.
public class MemShipStore : IShipStore
{
    public void Persist(string id)
    {
        Console.WriteLine(id);
    }
}

public class DbShipStore : IShipStore
{
    private readonly List<string> _rows = new List<string>();

    public void Persist(string id)
    {
        _rows.Add(id);
    }
}

public class ServiceRegistry
{
}

public class ShipModule
{
    private readonly IShipStore _store;

    public ShipModule(IShipStore store)
    {
        _store = store;
    }

    public void Enqueue(string id)
    {
        _store.Persist(id);
    }

    public static void Wire(ServiceRegistry services)
    {
        services.AddScoped<IShipStore, DbShipStore>();
    }
}
