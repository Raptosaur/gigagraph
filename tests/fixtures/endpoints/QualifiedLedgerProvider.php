<?php

namespace App\Providers;

class QualifiedLedgerProvider implements \Silex\ControllerProviderInterface
{
    public function connect($app)
    {
        $controllers = $app['controllers_factory'];
        $controllers->get('/qualified-ledgers', [$this, 'listQualified']);
        return $controllers;
    }

    public function listQualified()
    {
        return ['ledgers' => []];
    }
}
