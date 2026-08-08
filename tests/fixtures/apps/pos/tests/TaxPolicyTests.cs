using NUnit.Framework;
using Pos.Core;

namespace Pos.Tests;

[TestFixture]
public class TaxPolicyTests
{
    [SetUp]
    public void Setup()
    {
        Policy = new FlatTaxPolicy(0.2m);
    }

    private ITaxPolicy Policy { get; set; }

    [Test]
    public void RoundsToCents()
    {
        Assert.AreEqual(0.02m, Policy.TaxFor(0.11m));
    }

    [TestCase(10, 2)]
    public void ScalesLinearly(decimal subtotal, decimal expected)
    {
        Assert.AreEqual(expected, Policy.TaxFor(subtotal));
    }
}
