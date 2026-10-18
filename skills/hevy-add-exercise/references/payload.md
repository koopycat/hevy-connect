# Payload für eine eigene Übung

Die Quelle ist der im Repository gespeicherte offizielle
[OpenAPI-Contract](../../../docs/hevy-openapi.json). Prüfe bei Abweichungen den
[aktuellen offiziellen Contract](https://api.hevyapp.com/docs/swagger-ui-init.js),
ohne den Repo-Snapshot von Hand zu ändern.

## Fünf Erstellungsfelder

| Feld | Bedeutung |
| --- | --- |
| `title` | Eindeutiger, nicht leerer Übungsname |
| `exercise_type` | Erfassung von Gewicht, Wiederholungen, Dauer oder Distanz |
| `equipment_category` | Vorhandene Gerätekategorie, kein freier Gerätename |
| `muscle_group` | Eine Hauptmuskelgruppe |
| `other_muscles` | Weitere Muskelgruppen als Array, gegebenenfalls leer |

Die CLI verlangt alle fünf Felder. Die API nutzt den Envelope `exercise`;
die CLI akzeptiert auch das innere Objekt und ergänzt ihn selbst.

## Gültige Werte

`exercise_type`:

```text
weight_reps, reps_only, bodyweight_reps, bodyweight_assisted_reps,
duration, weight_duration, distance_duration, short_distance_weight
```

`equipment_category`:

```text
none, barbell, dumbbell, kettlebell, machine,
plate, resistance_band, suspension, other
```

`muscle_group` und alle Werte von `other_muscles`:

```text
abdominals, shoulders, biceps, triceps, forearms,
quadriceps, hamstrings, calves, glutes, abductors, adductors,
lats, upper_back, traps, lower_back, chest, cardio, neck, full_body, other
```

## Synthetisches Beispiel

Für eine bekannte Schulterpresse; den Namen und die Muskelwahl an den konkreten
Auftrag anpassen. Schulterpresse: Hauptmuskel `shoulders`, weiterer Muskel
`triceps`. Schrägbank-Brustpresse: Hauptmuskel `chest`, weitere Muskeln
`shoulders` und `triceps`.

```json
{
  "exercise": {
    "title": "Example Equipment Iso-Lateral Shoulder Press",
    "exercise_type": "weight_reps",
    "equipment_category": "machine",
    "muscle_group": "shoulders",
    "other_muscles": ["triceps"]
  }
}
```

## Read-back-Zuordnung

| Erstellungsfeld | Zurückgelesenes Feld |
| --- | --- |
| `title` | `title` |
| `exercise_type` | `type` |
| `equipment_category` | `equipment` |
| `muscle_group` | `primary_muscle_group` |
| `other_muscles` | `secondary_muscle_groups` |

Ein neuer eigener Eintrag muss außerdem `is_custom: true` zurückgeben.
`thumbnail_url` ist ein Lesefeld, kein dokumentiertes Erstellungsfeld.
