<?php

// Laravel 8 controller shapes referenced by api8_routes.php: a single-action
// (invokable) controller and a conventional controller with a project-unique
// method name for tuple-handler resolution.

namespace App\Http\Controllers;

class AppVersionController
{
    public function __invoke()
    {
        return ['version' => '1.0.0'];
    }
}

class CouponController
{
    public function redeem(string $code)
    {
        return ['redeemed' => $code];
    }
}
