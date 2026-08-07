using System;

namespace Acme.Store;

public interface IClock
{
    DateTime Now();
}

public class SystemClock : IClock
{
    public DateTime Now()
    {
        return DateTime.UtcNow;
    }
}

public class ServiceHost
{
    public void Log(string message)
    {
        Console.WriteLine(message);
    }
}

public class Registry
{
    public void Configure(ServiceHost services)
    {
        services.AddScoped<IClock, SystemClock>();
        services.AddSingleton<IClock, SystemClock>();
        services.AddTransient<IClock, SystemClock>();
        services.Log("wired");
        Pick<IClock>(null);
    }

    public T Pick<T>(object value)
    {
        return (T)value;
    }
}
