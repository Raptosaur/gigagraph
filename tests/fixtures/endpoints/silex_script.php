<?php

use Symfony\Component\HttpFoundation\Request;

$app->get('/lanterns', function () use ($app) {
    return $app['twig']->render('lanterns.html.twig');
})
->bind('lanterns')
;

$app->post('/lanterns/{id}/light', function (Request $request, $id) use ($app) {
    return ['lit' => $id];
});
