<?php

namespace App\Providers;

use Silex\Api\ControllerProviderInterface;
use Silex\Application;

class Ledger21Provider implements ControllerProviderInterface
{
    public function connect(Application $app)
    {
        $controllers = $app['controllers_factory'];

        $controllers
            ->get('/ledgers/{id}', [$this, 'showLedger'])
            ->assert('id', '\d+')
            ->convert('id', function ($id) { return (int) $id; })
            ->value('id', 1)
            ->bind('ledger_show')
            ->secure('ROLE_ADMIN');

        $controllers
            ->match('/ledgers/{id}/close', [$this, 'closeLedger'])
            ->method('POST|DELETE')
            ->assert('id', '\d+');

        $controllers->options('/ledgers', function () {
            return ['methods' => ['GET']];
        });

        $sub = $app['controllers_factory'];
        $sub->get('/entries', [$this, 'listEntries'])->before(function () {});
        $controllers->mount('/nested', $sub);

        return $controllers;
    }

    public function showLedger($id)
    {
        return ['ledger' => $id];
    }

    public function closeLedger($id)
    {
        return ['closed' => $id];
    }

    public function listEntries()
    {
        return ['entries' => []];
    }
}
