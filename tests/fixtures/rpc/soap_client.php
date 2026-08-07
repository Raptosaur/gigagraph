<?php

function fetchQuote(): float
{
    $client = new SoapClient("http://quotes.example.com/service?wsdl");
    return $client->__soapCall('getQuote', ['ACME']);
}
