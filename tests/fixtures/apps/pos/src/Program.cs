using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.DependencyInjection;
using Pos.Core;

var builder = WebApplication.CreateBuilder(args);
builder.Services.AddSingleton<ITaxPolicy, FlatTaxPolicy>();
builder.Services.AddScoped<Checkout>();

var app = builder.Build();

app.MapGet("/health", () => Results.Ok(new { status = "ok" }));
app.MapPost("/checkout/scan", (Checkout checkout, ScanRequest req) =>
{
    checkout.Scan(req.Sku, req.Quantity, req.UnitPrice);
    return Results.Accepted();
});
app.MapGet("/checkout/total", (Checkout checkout) => Results.Ok(checkout.Total()));

app.Run();

public record ScanRequest(string Sku, int Quantity, decimal UnitPrice);
