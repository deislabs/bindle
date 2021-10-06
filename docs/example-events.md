# Event Types

These are examples of events that can be emitted when running bindle with the --events flag.

## Invoice Created

```json
{
  "event_date": "2021-10-05T20:05:50.997960746Z",
  "event_data": {
    "InvoiceCreated": {
      "bindleVersion": "1.0.0",
      "yanked": null,
      "yankedSignature": null,
      "bindle": {
        "name": "enterprise.com/warpcore",
        "version": "1.0.0",
        "description": "Warp core components",
        "authors": [
          "Geordi La Forge <mycoolvisor@ufp.com>"
        ]
      },
      "annotations": {
        "engineering_location": "main"
      },
      "parcel": [
        {
          "label": {
            "sha256": "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5",
            "mediaType": "text/plain",
            "name": "isolinear_chip.txt",
            "size": 9,
            "annotations": null,
            "feature": null,
            "origin": null
          },
          "conditions": null
        }
      ],
      "group": null,
      "signature": [
        {
          "by": "Test <test@example.com>",
          "signature": "OxjhtGwTVDMJScoBDovNB1U52RsD2DyMgQoPIVAew0n4UKyY9Cw8S7KEkSsN6Lj71EFt8QPKO1hg1Tsz26MtBg==",
          "key": "ccjHo+plTirpq+QJQ40/vrVjrVIycjMQeoDwdZZsbW8=",
          "role": "host",
          "at": 1633464350
        }
      ]
    }
  }
}
```

## Missing Parcel

```json
{
  "event_date": "2021-10-05T20:05:51.013871309Z",
  "event_data": {
    "MissingParcel": {
      "sha256": "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5",
      "mediaType": "text/plain",
      "name": "isolinear_chip.txt",
      "size": 9,
      "annotations": null,
      "feature": null,
      "origin": null
    }
  }
}
```

## ParcelCreated

```json

{
  "event_date": "2021-10-05T20:05:51.028318270Z",
  "event_data": {
    "ParcelCreated": [
      "enterprise.com/warpcore/1.0.0",
      "23f310b54076878fd4c36f0c60ec92011a8b406349b98dd37d08577d17397de5"
    ]
  }
}
```

## Invoice Yanked

```json
{
  "event_date": "2021-10-05T20:05:51.028318270Z",
  "event_data": {
    "InvoiceYanked":"enterprise.com/warpcore/1.0.0"
  }
}
```
