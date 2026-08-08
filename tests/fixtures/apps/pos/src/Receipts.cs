using System;
using System.Text;
using System.Threading.Tasks;

namespace Pos.Core;

public static class Receipts
{
    public static string Render(Checkout checkout, string storeName)
    {
        var sb = new StringBuilder();
        sb.AppendLine(storeName);
        sb.AppendLine($"TOTAL {checkout.Total():C}");
        return sb.ToString();
    }

    public static async Task<string> RenderAsync(Checkout checkout, string storeName)
    {
        await Task.Yield();
        return Render(checkout, storeName);
    }

    private static string Line(string label, decimal amount) => $"{label,-20}{amount,8:C}";
}

public class ReceiptPrinter : IDisposable
{
    private bool _disposed;

    public void Print(string body) => Console.Write(body);

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
    }
}
