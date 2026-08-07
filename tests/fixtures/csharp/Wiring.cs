using System;
using System.Collections.Generic;

namespace Acme.Store;

public interface IUserStore
{
    void Save(string name);
}

public interface IAuditSource { }

public interface IAuditStore : IUserStore { }

public class DbUserStore : IUserStore
{
    private readonly List<string> _rows = new List<string>();

    public void Save(string name)
    {
        _rows.Add(name);
    }
}

public class MemoryStore : Acme.Store.IUserStore
{
    public void Save(string name) { }
}

public class UserService
{
    private readonly IUserStore _store;

    public IUserStore Store { get; }

    public UserService(IUserStore store)
    {
        _store = store;
        Store = store;
    }

    public void Register(string name)
    {
        _store.Save(name);
    }

    public static UserService Wire()
    {
        var store = new DbUserStore();
        DbUserStore backup = new DbUserStore();
        IUserStore fallback = new MemoryStore();
        UserService svc = new UserService(store);
        svc.Register("seed");
        backup.Save("cold");
        fallback.Save("warm");
        return svc;
    }
}

public class AuditService(IUserStore store) : IAuditSource
{
    private readonly IUserStore _audit = store;

    public void Flush() => _audit.Save("flush");
}

public record Wiring(IUserStore Store, string Label) : IAuditSource;

public struct Slot : IComparable<Slot>
{
    public int Rank;

    public int CompareTo(Slot other) => Rank - other.Rank;
}
