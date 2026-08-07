<?php

use Silex\Application;
use Symfony\Component\HttpFoundation\Request;

class GnomeController
{
    public function listGnomes(Request $request)
    {
        return ['gnomes' => []];
    }

    public function createGnome(Request $request)
    {
        return ['created' => true];
    }
}

$app = new Application();

$app->get('/gnomes', 'GnomeController::listGnomes');
$app->post('/gnomes', 'GnomeController::createGnome');
$app->put('/gnomes/{id}', function ($id) {
    return ['updated' => $id];
});
$app->match('/gnomes/ping', function () {
    return 'pong';
});
$app->match('/gnomes/toggle', 'GnomeController::createGnome')->method('GET|POST');

$controllers = $app['controllers_factory'];
$controllers->get('/hats', function () {
    return ['hats' => []];
});
$controllers->delete('/hats/{id}', 'GnomeController::listGnomes');
$controllers->match('/hats/{id}/rename', 'GnomeController::createGnome');

$app->mount('/workshop', $controllers);

$app->run();
