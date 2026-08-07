<?php

namespace App\Providers;

use App\Providers\BaseControllerProvider;

class ApiWidgetProvider extends BaseControllerProvider
{
    public function connect($app)
    {
        $controllers = $app['controllers_factory'];
        $controllers->get('/widget-registry', 'WidgetController::registry');
        return $controllers;
    }
}
