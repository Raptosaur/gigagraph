<?php

use Illuminate\Support\Facades\Route;

define('SHOP_BASE', '/shop');

Route::get('/orders/{id}', fn ($id) => $id);
Route::post('/orders', fn () => 'ok');
Route::match(['get', 'post'], '/mixed', fn () => 'ok');
Route::any('/anything', fn () => 'ok');
Route::resource('photos', PhotoController::class);
Route::get(SHOP_BASE . '/carts/{id}', fn ($id) => $id);

Route::prefix('/admin')->group(function () {
    Route::get('/stats', fn () => 'ok');
});

Route::prefix('/v2')->group(function () {
    Route::prefix('/beta')->group(function () {
        Route::get('/flags', fn () => 'ok');
    });
});

Route::group(['prefix' => '/legacy'], function () {
    Route::post('/import', fn () => 'ok');
});

Route::get('/cart/{id}', 'OldShopController@showCart');
