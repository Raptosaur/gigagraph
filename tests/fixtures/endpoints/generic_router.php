<?php

$router->get('/beacons', 'BeaconController::listBeacons');
$router->post('/beacons', function () { return 'created'; });
$router->put('/beacons/{id}', 'BeaconController::listBeacons')->name('beacon.update');

$f3->route('GET|POST /signals', function () { return 'signal'; });

$client->get('/not-a-route', ['timeout' => 5]);
