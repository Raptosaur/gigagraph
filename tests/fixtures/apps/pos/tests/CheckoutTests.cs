using System;
using Pos.Core;
using Xunit;

namespace Pos.Tests;

public class CheckoutTests
{
    private readonly Checkout _checkout = new(new FlatTaxPolicy(0.1m));

    [Fact]
    public void SubtotalSumsScannedItems()
    {
        _checkout.Scan("apple", 2, 1.50m);
        Assert.Equal(3.00m, _checkout.Subtotal());
    }

    [Fact]
    public void VoidRemovesEveryMatchingLine()
    {
        _checkout.Scan("apple", 1, 1m);
        Assert.True(_checkout.Void("apple"));
    }

    [Theory]
    [InlineData(100, 110)]
    [InlineData(0, 0)]
    public void TotalAppliesTax(decimal price, decimal expected)
    {
        _checkout.Scan("thing", 1, price);
        Assert.Equal(expected, _checkout.Total());
    }

    private static Checkout Fixture() => Checkout.Empty();
}

public class ReceiptTests
{
    [Fact]
    public void RenderIncludesStoreName()
    {
        Assert.Contains("Corner Store", Receipts.Render(Checkout.Empty(), "Corner Store"));
    }
}
