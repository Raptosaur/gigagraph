// Minimal-API shapes distilled from eShopOnWeb's PublicApi: slash-less
// route patterns, a `var`-bound MapGroup borrowed file-wide, and a chained
// MapGroup sharing its chain-start byte.
var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

var api = app.MapGroup("/minapi");
api.MapGet("orders-lite", () => "lite");
api.MapPost("orders-lite", () => "lite");

app.MapGet("healthz-lite", () => "ok");
app.MapGroup("/minapi").MapDelete("orders-lite/{id}", () => "gone");

app.Run();
