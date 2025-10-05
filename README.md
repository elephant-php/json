# elephant-php/net

PHP extension for simple JSON encoding and decoding where return exeption if has error 

```php
// Examle function usege
<?php

use function Elephant\Json;

$data = json_encode([
    'message' => 'hello world',
    'code' => 1122,
    'isAdmin' => true
]);

var_dump($data); //string(52) "{"message":"hello world","code":1122,"isAdmin":true}"
```
