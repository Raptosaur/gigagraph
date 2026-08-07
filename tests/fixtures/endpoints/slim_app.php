<?php

use Slim\Factory\AppFactory;

$app = AppFactory::create();

$app->get('/brews', function ($request, $response) {
    return $response;
});

$app->post('/brews', function ($request, $response) {
    return $response;
});

$app->run();
