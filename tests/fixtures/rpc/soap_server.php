<?php

function getQuote(string $symbol): float
{
    return 42.0;
}

class LegacyApi
{
    public function ping(): string
    {
        return "pong";
    }
}

$server = new SoapServer(null, ['uri' => 'urn:quotes']);
$server->addFunction('getQuote');
$server->setClass('LegacyApi');
$server->handle();
