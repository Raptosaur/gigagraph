<?php

namespace App\Controller;

class AlmanacController
{
    public function listAlmanacs()
    {
        return ['almanacs' => []];
    }

    public function createAlmanac()
    {
        return ['created' => true];
    }
}

class HealthController
{
    public function __invoke()
    {
        return 'ok';
    }
}
