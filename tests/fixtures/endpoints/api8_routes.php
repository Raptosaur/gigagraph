<?php

// Laravel 6-8 era route shapes, distilled from real apps (validated against
// crater-invoice/crater and the laravel/laravel v8.6.12 skeleton):
// - Route::middleware([...])->prefix('/setup')->group(...): MID-chain
//   prefix — the prefix call is not chain-initial, so it arrives
//   receiver-less; pairing is by shared chain-start byte (crater's
//   routes/api.php installation block).
// - Route::middleware('auth')->get('/whoami', ...): chained VERB call
//   (Laravel 8 skeleton's api.php shape) — also receiver-less.
// - Route::post('signin', ...): slash-less URI (crater's auth block).
// - Route::apiResource inside a prefix group: 5-route expansion (no
//   create/edit forms) joined with the enclosing group prefix.
// - [CouponController::class, 'redeem'] tuple handler: the class const sits
//   below harvest depth (probe-verified), so resolution is by project-unique
//   method name.
// - AppVersionController::class single-action (invokable) handler: the bare
//   ::class const arrives as an Ident pair, resolved to __invoke.

use App\Http\Controllers\AppVersionController;
use App\Http\Controllers\CouponController;
use Illuminate\Support\Facades\Route;

Route::middleware(['installed'])->prefix('/setup')->group(function () {
    Route::get('/steps', fn () => 'ok');
});

Route::middleware('auth')->get('/whoami', fn () => 'me');

Route::post('signin', [CouponController::class, 'redeem']);

Route::get('/app-version', AppVersionController::class);

Route::post('/coupons/redeem', [CouponController::class, 'redeem']);

Route::prefix('/api/v1')->group(function () {
    Route::apiResource('coupons', CouponController::class);
});
