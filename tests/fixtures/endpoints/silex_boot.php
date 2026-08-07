<?php

use Silex\Application;

$app = new Application();
$app->mount('/api/crates', new CrateControllerProvider());
$app->run();
