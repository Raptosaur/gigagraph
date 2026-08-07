<?php

class CrateController
{
    public function listCrates()
    {
        return ['crates' => []];
    }
}

class CrateControllerProvider implements ControllerProviderInterface
{
    public function connect($app)
    {
        $controllers = $app['controllers_factory'];

        $controllers->get('', 'crate.controller:listCrates');
        $controllers->put('/{id}', function ($id) {
            return ['crate' => $id];
        })->bind('crate_update');

        return $controllers;
    }
}
