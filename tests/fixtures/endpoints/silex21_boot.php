<?php

use Silex\Application;

$app = new Application();
$app->mount('/v2', new Ledger21Provider());
$app->run();
