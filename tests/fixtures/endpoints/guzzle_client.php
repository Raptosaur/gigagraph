<?php

use GuzzleHttp\Client;

define('REPORTS_BASE', '/admin/api');

function fetchReports(Client $client)
{
    return $client->get(REPORTS_BASE . '/reports');
}
