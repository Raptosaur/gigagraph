using System;
using System.Collections.Generic;
using System.Linq;

namespace Pos.Core;

public record LineItem(string Sku, int Quantity, decimal UnitPrice)
{
    public decimal Subtotal => Quantity * UnitPrice;
}

public interface ITaxPolicy
{
    decimal TaxFor(decimal subtotal);
}

public sealed class FlatTaxPolicy : ITaxPolicy
{
    private readonly decimal _rate;

    public FlatTaxPolicy(decimal rate) => _rate = rate;

    public decimal TaxFor(decimal subtotal) => Math.Round(subtotal * _rate, 2);
}

public class Checkout
{
    private readonly List<LineItem> _items = new();
    private readonly ITaxPolicy _tax;

    public Checkout(ITaxPolicy tax)
    {
        _tax = tax;
    }

    public void Scan(string sku, int quantity, decimal unitPrice)
    {
        _items.Add(new LineItem(sku, quantity, unitPrice));
    }

    public bool Void(string sku)
    {
        return _items.RemoveAll(i => i.Sku == sku) > 0;
    }

    public decimal Subtotal() => _items.Sum(i => i.Subtotal);

    public decimal Total()
    {
        var subtotal = Subtotal();
        return subtotal + _tax.TaxFor(subtotal);
    }

    public static Checkout Empty() => new(new FlatTaxPolicy(0m));
}
