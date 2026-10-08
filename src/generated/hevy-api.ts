// Code generated from docs/hevy-openapi.json by scripts/generate-api-types.ts. DO NOT EDIT.
// Regenerate with `just api-types`.
export interface paths {
    "/v1/body_measurements": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a paginated list of body measurements for the authenticated user */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items per page (Max 10) */
                    pageSize?: number;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of body measurements */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            body_measurements?: components["schemas"]["BodyMeasurement"][];
                            /** @example 1 */
                            page?: number;
                            /** @example 5 */
                            page_count?: number;
                        };
                    };
                };
                /** @description Invalid page or pageSize */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
                /** @description Page not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        /** Create a body measurement entry for a given date. Returns 409 if an entry already exists for that date. */
        post: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["BodyMeasurement"];
                };
            };
            responses: {
                /** @description The measurement was successfully created. The response body is empty. */
                201: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            error?: string;
                        };
                    };
                };
                /** @description A measurement for this date already exists */
                409: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            error?: string;
                        };
                    };
                };
            };
        };
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/body_measurements/{date}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a single body measurement by date */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The date of the body measurement (YYYY-MM-DD) */
                    date: string;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description The body measurement for the given date */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["BodyMeasurement"];
                    };
                };
                /** @description Body measurement not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            error?: string;
                        };
                    };
                };
            };
        };
        /** Update an existing body measurement entry for a given date. All fields are overwritten; omitted fields are set to null. */
        put: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The date of the measurement to update (YYYY-MM-DD) */
                    date: string;
                };
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PutBodyMeasurement"];
                };
            };
            responses: {
                /** @description The measurement was successfully updated */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            error?: string;
                        };
                    };
                };
                /** @description No measurement found for the given date */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            error?: string;
                        };
                    };
                };
            };
        };
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/exercise_history/{exerciseTemplateId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get exercise history for a specific exercise template */
        get: {
            parameters: {
                query?: {
                    /**
                     * @description Optional end date for filtering exercise history (ISO 8601 format)
                     * @example 2024-12-31T23:59:59Z
                     */
                    end_date?: string;
                    /**
                     * @description Optional start date for filtering exercise history (ISO 8601 format)
                     * @example 2024-01-01T00:00:00Z
                     */
                    start_date?: string;
                };
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the exercise template */
                    exerciseTemplateId: string;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A list of exercise history entries */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            exercise_history?: components["schemas"]["ExerciseHistoryEntry"][];
                        };
                    };
                };
                /** @description Invalid request parameters or date format */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/exercise_templates": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a paginated list of exercise templates available on the account. */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items on the requested page (Max 100) */
                    pageSize?: number;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of exercise templates */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            exercise_templates?: components["schemas"]["ExerciseTemplate"][];
                            /**
                             * @description Current page number
                             * @default 1
                             */
                            page: number;
                            /**
                             * @description Total number of pages
                             * @default 5
                             */
                            page_count: number;
                        };
                    };
                };
                /** @description Invalid page size */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        /** Create a new custom exercise template. */
        post: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            /** @description The exercise template to create. */
            requestBody: {
                content: {
                    "application/json": components["schemas"]["CreateCustomExerciseRequestBody"];
                };
            };
            responses: {
                /** @description The exercise template was successfully created */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description The ID of the exercise template
                             * @example 123
                             */
                            id?: number;
                        };
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description Error message
                             * @example Invalid request body
                             */
                            error?: string;
                        };
                    };
                };
                /** @description Exceeds custom exercise limit */
                403: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description Error message
                             * @example exceeds-custom-exercise-limit
                             */
                            error?: string;
                        };
                    };
                };
            };
        };
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/exercise_templates/{exerciseTemplateId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a single exercise template by id. */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the exercise template */
                    exerciseTemplateId: string;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description Success */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["ExerciseTemplate"];
                    };
                };
                /** @description Exercise template not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/routine_folders": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a paginated list of routine folders available on the account. */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items on the requested page (Max 10) */
                    pageSize?: number;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of routine folders */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description Current page number
                             * @default 1
                             */
                            page: number;
                            /**
                             * @description Total number of pages
                             * @default 5
                             */
                            page_count: number;
                            routine_folders?: components["schemas"]["RoutineFolder"][];
                        };
                    };
                };
                /** @description Invalid page size */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        /** Create a new routine folder. The folder will be created at index 0, and all other folders will have their indexes incremented. */
        post: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PostRoutineFolderRequestBody"];
                };
            };
            responses: {
                /** @description The routine folder was successfully created */
                201: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["RoutineFolder"];
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/routine_folders/{folderId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a single routine folder by id. */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the routine folder */
                    folderId: number;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description Success */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["RoutineFolder"];
                    };
                };
                /** @description Routine folder not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/routines": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a paginated list of routines */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items on the requested page (Max 10) */
                    pageSize?: number;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of routines */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description Current page number
                             * @example 1
                             */
                            page?: number;
                            /**
                             * @description Total number of pages
                             * @example 5
                             */
                            page_count?: number;
                            routines?: components["schemas"]["Routine"][];
                        };
                    };
                };
                /** @description Invalid page size */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        /** Create a new routine */
        post: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PostRoutinesRequestBody"];
                };
            };
            responses: {
                /** @description The routine was successfully created */
                201: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            routine?: components["schemas"]["Routine"];
                        };
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
                /** @description Routine limit exceeded */
                403: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/routines/{routineId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a routine by its Id */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the routine */
                    routineId: string;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description The routine with the provided id */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            routine?: components["schemas"]["Routine"];
                        };
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        /** Update an existing routine */
        put: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the routine */
                    routineId: string;
                };
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PutRoutinesRequestBody"];
                };
            };
            responses: {
                /** @description The routine was successfully updated */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            routine?: components["schemas"]["Routine"];
                        };
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
                /** @description Routine doesn't exist or doesn't belong to the user */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/user/info": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get user info */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description The authenticated user's info */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["UserInfoResponse"];
                    };
                };
                /** @description User not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workouts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a paginated list of workouts */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items on the requested page (Max 10) */
                    pageSize?: number;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of workouts */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description Current page number
                             * @example 1
                             */
                            page?: number;
                            /**
                             * @description Total number of pages
                             * @example 5
                             */
                            page_count?: number;
                            workouts?: components["schemas"]["Workout"][];
                        };
                    };
                };
                /** @description Invalid page size */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        /** Create a new workout */
        post: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PostWorkoutsRequestBody"];
                };
            };
            responses: {
                /** @description The workout was successfully created */
                201: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description The created workout, returned as a single-element array. */
                            workout?: components["schemas"]["Workout"][];
                        };
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workouts/{workoutId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get a single workout’s complete details by the workoutId */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the workout */
                    workoutId: string;
                };
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description Success */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["Workout"];
                    };
                };
                /** @description Workout not found */
                404: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        /** Update an existing workout */
        put: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path: {
                    /** @description The id of the workout */
                    workoutId: string;
                };
                cookie?: never;
            };
            requestBody: {
                content: {
                    "application/json": components["schemas"]["PostWorkoutsRequestBody"];
                };
            };
            responses: {
                /** @description The workout was successfully updated */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["Workout"];
                    };
                };
                /** @description Invalid request body */
                400: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /** @description Error message */
                            error?: string;
                        };
                    };
                };
            };
        };
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workouts/count": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Get the total number of workouts on the account */
        get: {
            parameters: {
                query?: never;
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description The total count of workouts */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": {
                            /**
                             * @description The total number of workouts
                             * @default 42
                             */
                            workout_count: number;
                        };
                    };
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/workouts/events": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Retrieve a paged list of workout events (updates or deletes) since a given date. Events are ordered from newest to oldest. The intention is to allow clients to keep their local cache of workouts up to date without having to fetch the entire list of workouts.
         * @description Returns a paginated array of workout events, indicating updates or deletions.
         */
        get: {
            parameters: {
                query?: {
                    /** @description Page number (Must be 1 or greater) */
                    page?: number;
                    /** @description Number of items on the requested page (Max 10) */
                    pageSize?: number;
                    since?: string;
                };
                header: {
                    "api-key": string;
                };
                path?: never;
                cookie?: never;
            };
            requestBody?: never;
            responses: {
                /** @description A paginated list of workout events */
                200: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content: {
                        "application/json": components["schemas"]["PaginatedWorkoutEvents"];
                    };
                };
                /** @description Internal Server Error */
                500: {
                    headers: {
                        [name: string]: unknown;
                    };
                    content?: never;
                };
            };
        };
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        BodyMeasurement: {
            /** @example 85 */
            abdomen?: number | null;
            /** @example 95 */
            chest_cm?: number | null;
            /**
             * Format: date
             * @example 2024-08-14
             */
            date: string;
            /** @example 18.5 */
            fat_percent?: number | null;
            /** @example 95 */
            hips?: number | null;
            /** @example 65 */
            lean_mass_kg?: number | null;
            /** @example 35 */
            left_bicep_cm?: number | null;
            /** @example 37 */
            left_calf?: number | null;
            /** @example 28 */
            left_forearm_cm?: number | null;
            /** @example 55 */
            left_thigh?: number | null;
            /** @example 38 */
            neck_cm?: number | null;
            /** @example 35.5 */
            right_bicep_cm?: number | null;
            /** @example 37.5 */
            right_calf?: number | null;
            /** @example 28.5 */
            right_forearm_cm?: number | null;
            /** @example 55.5 */
            right_thigh?: number | null;
            /** @example 115 */
            shoulder_cm?: number | null;
            /** @example 80 */
            waist?: number | null;
            /** @example 80.5 */
            weight_kg?: number | null;
        };
        CreateCustomExerciseRequestBody: {
            exercise: {
                /**
                 * @description The equipment category of the exercise template.
                 * @example barbell
                 */
                equipment_category?: components["schemas"]["EquipmentCategory"];
                exercise_type?: components["schemas"]["CustomExerciseType"];
                /**
                 * @description The muscle group of the exercise template.
                 * @example chest
                 */
                muscle_group?: components["schemas"]["MuscleGroup"];
                /**
                 * @description The other muscles of the exercise template.
                 * @example [
                 *       "biceps",
                 *       "triceps"
                 *     ]
                 */
                other_muscles?: components["schemas"]["MuscleGroup"][];
                /**
                 * @description The title of the exercise template.
                 * @example Bench Press
                 */
                title?: string;
            };
        };
        /**
         * @example weight_reps
         * @enum {string}
         */
        CustomExerciseType: "weight_reps" | "reps_only" | "bodyweight_reps" | "bodyweight_assisted_reps" | "duration" | "weight_duration" | "distance_duration" | "short_distance_weight";
        DeletedWorkout: {
            /**
             * @description A date string indicating when the workout was deleted
             * @example 2021-09-13T12:00:00Z
             */
            deleted_at?: string;
            /**
             * @description The unique identifier of the deleted workout
             * @example efe6801c-4aee-4959-bcdd-fca3f272821b
             */
            id: string;
            /**
             * @description Indicates the type of the event (deleted)
             * @example deleted
             */
            type: string;
        };
        /**
         * @example barbell
         * @enum {string}
         */
        EquipmentCategory: "none" | "barbell" | "dumbbell" | "kettlebell" | "machine" | "plate" | "resistance_band" | "suspension" | "other";
        Exercise: {
            /**
             * @description The id of the exercise template. This can be used to fetch the exercise template.
             * @example 05293BCA
             */
            exercise_template_id?: string;
            /**
             * @description Index indicating the order of the exercise in the workout.
             * @example 0
             */
            index?: number;
            /**
             * @description Notes on the exercise
             * @example Paid closer attention to form today. Felt great!
             */
            notes?: string;
            sets?: components["schemas"]["Set"][];
            /**
             * @description The id of the superset that the exercise belongs to. A value of null indicates the exercise is not part of a superset.
             * @example 0
             */
            superset_id?: number | null;
            /**
             * @description Title of the exercise
             * @example Bench Press (Barbell)
             */
            title?: string;
        };
        ExerciseHistoryEntry: {
            /**
             * @description A custom metric for the set
             * @example null
             */
            custom_metric?: number | null;
            /**
             * @description The distance in meters
             * @example null
             */
            distance_meters?: number | null;
            /**
             * @description The duration in seconds
             * @example null
             */
            duration_seconds?: number | null;
            /**
             * @description The exercise template ID
             * @example D04AC939
             */
            exercise_template_id?: string;
            /**
             * @description The number of repetitions
             * @example 10
             */
            reps?: number | null;
            /**
             * @description The Rating of Perceived Exertion
             * @example 8.5
             */
            rpe?: number | null;
            /**
             * @description The type of set (warmup, normal, failure, dropset)
             * @example normal
             */
            set_type?: string;
            /**
             * @description The weight in kilograms
             * @example 100
             */
            weight_kg?: number | null;
            /**
             * @description ISO 8601 timestamp of when the workout was recorded to have ended.
             * @example 2024-01-01T13:00:00Z
             */
            workout_end_time?: string;
            /**
             * @description The workout ID
             * @example b459cba5-cd6d-463c-abd6-54f8eafcadcb
             */
            workout_id?: string;
            /**
             * @description ISO 8601 timestamp of when the workout was recorded to have started.
             * @example 2024-01-01T12:00:00Z
             */
            workout_start_time?: string;
            /**
             * @description The workout title
             * @example Morning Workout 💪
             */
            workout_title?: string;
        };
        ExerciseTemplate: {
            equipment?: components["schemas"]["EquipmentCategory"];
            /**
             * @description The exercise template ID.
             * @example b459cba5-cd6d-463c-abd6-54f8eafcadcb
             */
            id?: string;
            /**
             * @description A boolean indicating whether the exercise is a custom exercise.
             * @example false
             */
            is_custom?: boolean;
            /**
             * @description The primary muscle group of the exercise.
             * @example chest
             */
            primary_muscle_group?: string;
            /** @description The secondary muscle groups of the exercise. */
            secondary_muscle_groups?: string[];
            /**
             * @description URL of the exercise's thumbnail image, or null if the exercise has none.
             * @example null
             */
            thumbnail_url?: string | null;
            /**
             * @description The exercise title.
             * @example Bench Press (Barbell)
             */
            title?: string;
            /**
             * @description The exercise type.
             * @example weight_reps
             */
            type?: string;
        };
        /**
         * @example chest
         * @enum {string}
         */
        MuscleGroup: "abdominals" | "shoulders" | "biceps" | "triceps" | "forearms" | "quadriceps" | "hamstrings" | "calves" | "glutes" | "abductors" | "adductors" | "lats" | "upper_back" | "traps" | "lower_back" | "chest" | "cardio" | "neck" | "full_body" | "other";
        PaginatedWorkoutEvents: {
            /** @description An array of workout events (either updated or deleted) */
            events: (components["schemas"]["UpdatedWorkout"] | components["schemas"]["DeletedWorkout"])[];
            /**
             * @description The current page number
             * @example 1
             */
            page: number;
            /**
             * @description The total number of pages available
             * @example 5
             */
            page_count: number;
        };
        PostRoutineFolderRequestBody: {
            routine_folder?: {
                /**
                 * @description The title of the routine folder.
                 * @example Push Pull 🏋️‍♂️
                 */
                title?: string;
            };
        };
        PostRoutinesRequestBody: {
            routine?: {
                exercises?: components["schemas"]["PostRoutinesRequestExercise"][];
                /**
                 * @description The folder id the routine should be added to. Pass null to insert the routine into default "My Routines" folder
                 * @example null
                 */
                folder_id?: number | null;
                /**
                 * @description Additional notes for the routine.
                 * @example Focus on form over weight. Remember to stretch.
                 */
                notes?: string;
                /**
                 * @description The title of the routine.
                 * @example April Leg Day 🔥
                 */
                title?: string;
            };
        };
        PostRoutinesRequestExercise: {
            /**
             * @description The ID of the exercise template.
             * @example D04AC939
             */
            exercise_template_id?: string;
            /**
             * @description Additional notes for the exercise.
             * @example Stay slow and controlled.
             */
            notes?: string | null;
            /**
             * @description The rest time in seconds.
             * @example 90
             */
            rest_seconds?: number | null;
            sets?: components["schemas"]["PostRoutinesRequestSet"][];
            /**
             * @description The ID of the superset.
             * @example null
             */
            superset_id?: number | null;
        };
        PostRoutinesRequestSet: {
            /**
             * @description A custom metric for the set. Currently used for steps and floors.
             * @example null
             */
            custom_metric?: number | null;
            /**
             * @description The distance in meters.
             * @example null
             */
            distance_meters?: number | null;
            /**
             * @description The duration in seconds.
             * @example null
             */
            duration_seconds?: number | null;
            /** @description Range of reps for the set, if applicable */
            rep_range?: {
                /**
                 * @description Ending rep count for the range
                 * @example 12
                 */
                end?: number;
                /**
                 * @description Starting rep count for the range
                 * @example 8
                 */
                start?: number;
            } | null;
            /**
             * @description The number of repetitions.
             * @example 10
             */
            reps?: number | null;
            /**
             * @description The type of the set.
             * @example normal
             * @enum {string}
             */
            type?: "warmup" | "normal" | "failure" | "dropset";
            /**
             * @description The weight in kilograms.
             * @example 100
             */
            weight_kg?: number | null;
        };
        PostWorkoutsRequestBody: {
            workout: {
                /**
                 * @description A description for the workout workout.
                 * @example Medium intensity leg day focusing on quads.
                 */
                description?: string | null;
                /**
                 * @description The time the workout ended.
                 * @example 2024-08-14T12:30:00Z
                 */
                end_time: string;
                exercises: components["schemas"]["PostWorkoutsRequestExercise"][];
                /**
                 * @description A boolean indicating if the workout is private. Optional - defaults to false when omitted.
                 * @default false
                 * @example false
                 */
                is_private: boolean;
                /**
                 * @description The time the workout started.
                 * @example 2024-08-14T12:00:00Z
                 */
                start_time: string;
                /**
                 * @description The title of the workout.
                 * @example Friday Leg Day 🔥
                 */
                title: string;
            };
        };
        PostWorkoutsRequestExercise: {
            /**
             * @description The ID of the exercise template.
             * @example D04AC939
             */
            exercise_template_id?: string;
            /**
             * @description Additional notes for the exercise.
             * @example Felt good today. Form was on point.
             */
            notes?: string | null;
            sets?: components["schemas"]["PostWorkoutsRequestSet"][];
            /**
             * @description The ID of the superset.
             * @example null
             */
            superset_id?: number | null;
        };
        PostWorkoutsRequestSet: {
            /**
             * @description A custom metric for the set. Currently used for steps and floors.
             * @example null
             */
            custom_metric?: number | null;
            /**
             * @description The distance in meters.
             * @example null
             */
            distance_meters?: number | null;
            /**
             * @description The duration in seconds.
             * @example null
             */
            duration_seconds?: number | null;
            /**
             * @description The number of repetitions.
             * @example 10
             */
            reps?: number | null;
            /**
             * @description The Rating of Perceived Exertion (RPE).
             * @example null
             * @enum {number|null}
             */
            rpe?: 6 | 7 | 7.5 | 8 | 8.5 | 9 | 9.5 | 10 | null;
            /**
             * @description The type of the set.
             * @example normal
             * @enum {string}
             */
            type?: "warmup" | "normal" | "failure" | "dropset";
            /**
             * @description The weight in kilograms.
             * @example 100
             */
            weight_kg?: number | null;
        };
        PutBodyMeasurement: {
            /** @example 85 */
            abdomen?: number | null;
            /** @example 95 */
            chest_cm?: number | null;
            /** @example 18.5 */
            fat_percent?: number | null;
            /** @example 95 */
            hips?: number | null;
            /** @example 65 */
            lean_mass_kg?: number | null;
            /** @example 35 */
            left_bicep_cm?: number | null;
            /** @example 37 */
            left_calf?: number | null;
            /** @example 28 */
            left_forearm_cm?: number | null;
            /** @example 55 */
            left_thigh?: number | null;
            /** @example 38 */
            neck_cm?: number | null;
            /** @example 35.5 */
            right_bicep_cm?: number | null;
            /** @example 37.5 */
            right_calf?: number | null;
            /** @example 28.5 */
            right_forearm_cm?: number | null;
            /** @example 55.5 */
            right_thigh?: number | null;
            /** @example 115 */
            shoulder_cm?: number | null;
            /** @example 80 */
            waist?: number | null;
            /** @example 80.5 */
            weight_kg?: number | null;
        };
        PutRoutinesRequestBody: {
            routine?: {
                exercises?: components["schemas"]["PutRoutinesRequestExercise"][];
                /**
                 * @description The folder id the routine should be added to. Pass null to insert the routine into default "My Routines" folder
                 * @example null
                 */
                folder_id?: number | null;
                /**
                 * @description Additional notes for the routine.
                 * @example Focus on form over weight. Remember to stretch.
                 */
                notes?: string | null;
                /**
                 * @description The title of the routine.
                 * @example April Leg Day 🔥
                 */
                title?: string;
            };
        };
        PutRoutinesRequestExercise: {
            /**
             * @description The ID of the exercise template.
             * @example D04AC939
             */
            exercise_template_id?: string;
            /**
             * @description Additional notes for the exercise.
             * @example Stay slow and controlled.
             */
            notes?: string | null;
            /**
             * @description The rest time in seconds.
             * @example 90
             */
            rest_seconds?: number | null;
            sets?: components["schemas"]["PutRoutinesRequestSet"][];
            /**
             * @description The ID of the superset.
             * @example null
             */
            superset_id?: number | null;
        };
        PutRoutinesRequestSet: {
            /**
             * @description A custom metric for the set. Currently used for steps and floors.
             * @example null
             */
            custom_metric?: number | null;
            /**
             * @description The distance in meters.
             * @example null
             */
            distance_meters?: number | null;
            /**
             * @description The duration in seconds.
             * @example null
             */
            duration_seconds?: number | null;
            /** @description Range of reps for the set, if applicable */
            rep_range?: {
                /**
                 * @description Ending rep count for the range
                 * @example 12
                 */
                end?: number | null;
                /**
                 * @description Starting rep count for the range
                 * @example 8
                 */
                start?: number | null;
            } | null;
            /**
             * @description The number of repetitions.
             * @example 10
             */
            reps?: number | null;
            /**
             * @description The type of the set.
             * @example normal
             * @enum {string}
             */
            type?: "warmup" | "normal" | "failure" | "dropset";
            /**
             * @description The weight in kilograms.
             * @example 100
             */
            weight_kg?: number | null;
        };
        Routine: {
            /**
             * @description ISO 8601 timestamp of when the routine was created.
             * @example 2021-09-14T12:00:00Z
             */
            created_at?: string;
            exercises?: {
                /**
                 * @description The id of the exercise template. This can be used to fetch the exercise template.
                 * @example 05293BCA
                 */
                exercise_template_id?: string;
                /**
                 * @description Index indicating the order of the exercise in the routine.
                 * @example 0
                 */
                index?: number;
                /**
                 * @description Routine notes on the exercise
                 * @example Focus on form. Go down to 90 degrees.
                 */
                notes?: string | null;
                /**
                 * @description The rest time in seconds between sets of the exercise
                 * @example 60
                 */
                rest_seconds?: number | null;
                sets?: {
                    /**
                     * @description Custom metric logged for the set (Currently only used to log floors or steps for stair machine exercises)
                     * @example 50
                     */
                    custom_metric?: number | null;
                    /**
                     * @description Number of meters logged for the set
                     * @example null
                     */
                    distance_meters?: number | null;
                    /**
                     * @description Number of seconds logged for the set
                     * @example null
                     */
                    duration_seconds?: number | null;
                    /**
                     * @description Index indicating the order of the set in the routine.
                     * @example 0
                     */
                    index?: number;
                    /** @description Range of reps for the set, if applicable */
                    rep_range?: {
                        /**
                         * @description Ending rep count for the range
                         * @example 12
                         */
                        end?: number | null;
                        /**
                         * @description Starting rep count for the range
                         * @example 8
                         */
                        start?: number | null;
                    } | null;
                    /**
                     * @description Number of reps logged for the set
                     * @example 10
                     */
                    reps?: number | null;
                    /**
                     * @description The type of set. This can be one of 'normal', 'warmup', 'dropset', 'failure'
                     * @example normal
                     */
                    type?: string;
                    /**
                     * @description Weight lifted in kilograms.
                     * @example 100
                     */
                    weight_kg?: number | null;
                }[];
                /**
                 * @description The id of the superset that the exercise belongs to. A value of null indicates the exercise is not part of a superset.
                 * @example 0
                 */
                superset_id?: number | null;
                /**
                 * @description Title of the exercise
                 * @example Bench Press (Barbell)
                 */
                title?: string;
            }[];
            /**
             * @description The routine folder ID.
             * @example 42
             */
            folder_id?: number | null;
            /**
             * @description The routine ID.
             * @example b459cba5-cd6d-463c-abd6-54f8eafcadcb
             */
            id?: string;
            /**
             * @description The routine title.
             * @example Upper Body 💪
             */
            title?: string;
            /**
             * @description ISO 8601 timestamp of when the routine was last updated.
             * @example 2021-09-14T12:00:00Z
             */
            updated_at?: string;
        };
        RoutineFolder: {
            /**
             * @description ISO 8601 timestamp of when the folder was created.
             * @example 2021-09-14T12:00:00Z
             */
            created_at?: string;
            /**
             * @description The routine folder ID.
             * @example 42
             */
            id?: number;
            /**
             * @description The routine folder index. Describes the order of the folder in the list.
             * @example 1
             */
            index?: number;
            /**
             * @description The routine folder title.
             * @example Push Pull 🏋️‍♂️
             */
            title?: string;
            /**
             * @description ISO 8601 timestamp of when the folder was last updated.
             * @example 2021-09-14T12:00:00Z
             */
            updated_at?: string;
        };
        Set: {
            /**
             * @description Custom metric logged for the set (Currently only used to log floors or steps for stair machine exercises)
             * @example 50
             */
            custom_metric?: number | null;
            /**
             * @description Number of meters logged for the set
             * @example null
             */
            distance_meters?: number | null;
            /**
             * @description Number of seconds logged for the set
             * @example null
             */
            duration_seconds?: number | null;
            /**
             * @description Index indicating the order of the set in the workout.
             * @example 0
             */
            index?: number;
            /**
             * @description Number of reps logged for the set
             * @example 10
             */
            reps?: number | null;
            /**
             * @description RPE (Relative perceived exertion) value logged for the set
             * @example 9.5
             */
            rpe?: number | null;
            /**
             * @description The type of set. This can be one of 'normal', 'warmup', 'dropset', 'failure'
             * @example normal
             */
            type?: string;
            /**
             * @description Weight lifted in kilograms.
             * @example 100
             */
            weight_kg?: number | null;
        };
        UpdatedWorkout: {
            /**
             * @description Indicates the type of the event (updated)
             * @example updated
             */
            type: string;
            workout: components["schemas"]["Workout"];
        };
        UserInfo: {
            /**
             * @description The user's preferred distance unit. Defaults to kilometers if unset.
             * @example kilometers
             * @enum {string}
             */
            distance_unit?: "kilometers" | "miles";
            /**
             * @description The user ID.
             * @example 9c465af3-de7d-42bc-9c7c-f0170396358b
             */
            id?: string;
            /**
             * @description The user's display name.
             * @example John doe
             */
            name?: string;
            /**
             * @description The user's public profile URL.
             * @example https://hevy.com/user/jhon
             */
            url?: string;
            /**
             * @description The user's username.
             * @example jhon
             */
            username?: string;
            /**
             * @description The user's preferred weight unit. Defaults to kg if unset.
             * @example kg
             * @enum {string}
             */
            weight_unit?: "kg" | "lbs";
        };
        UserInfoResponse: {
            data?: components["schemas"]["UserInfo"];
        };
        Workout: {
            /**
             * @description ISO 8601 timestamp of when the workout was created.
             * @example 2021-09-14T12:00:00Z
             */
            created_at?: string;
            /**
             * @description The workout description.
             * @example Pushed myself to the limit today!
             */
            description?: string | null;
            /**
             * @description ISO 8601 timestamp of when the workout was recorded to have ended.
             * @example 2021-09-14T12:00:00Z
             */
            end_time?: string;
            exercises?: {
                /**
                 * @description The id of the exercise template. This can be used to fetch the exercise template.
                 * @example 05293BCA
                 */
                exercise_template_id?: string;
                /**
                 * @description Index indicating the order of the exercise in the workout.
                 * @example 0
                 */
                index?: number;
                /**
                 * @description Notes on the exercise
                 * @example Paid closer attention to form today. Felt great!
                 */
                notes?: string | null;
                sets?: {
                    /**
                     * @description Custom metric logged for the set (Currently only used to log floors or steps for stair machine exercises)
                     * @example 50
                     */
                    custom_metric?: number | null;
                    /**
                     * @description Number of meters logged for the set
                     * @example null
                     */
                    distance_meters?: number | null;
                    /**
                     * @description Number of seconds logged for the set
                     * @example null
                     */
                    duration_seconds?: number | null;
                    /**
                     * @description Index indicating the order of the set in the workout.
                     * @example 0
                     */
                    index?: number;
                    /**
                     * @description Number of reps logged for the set
                     * @example 10
                     */
                    reps?: number | null;
                    /**
                     * @description RPE (Relative perceived exertion) value logged for the set
                     * @example 9.5
                     */
                    rpe?: number | null;
                    /**
                     * @description The type of set. This can be one of 'normal', 'warmup', 'dropset', 'failure'
                     * @example normal
                     */
                    type?: string;
                    /**
                     * @description Weight lifted in kilograms.
                     * @example 100
                     */
                    weight_kg?: number | null;
                }[];
                /**
                 * @description The id of the superset that the exercise belongs to. A value of null indicates the exercise is not part of a superset.
                 * @example 0
                 */
                superset_id?: number | null;
                /**
                 * @description Title of the exercise
                 * @example Bench Press (Barbell)
                 */
                title?: string;
            }[];
            /**
             * @description The workout ID.
             * @example b459cba5-cd6d-463c-abd6-54f8eafcadcb
             */
            id?: string;
            /**
             * @description The ID of the routine that this workout belongs to.
             * @example b459cba5-cd6d-463c-abd6-54f8eafcadcb
             */
            routine_id?: string | null;
            /**
             * @description ISO 8601 timestamp of when the workout was recorded to have started.
             * @example 2021-09-14T12:00:00Z
             */
            start_time?: string;
            /**
             * @description The workout title.
             * @example Morning Workout 💪
             */
            title?: string;
            /**
             * @description ISO 8601 timestamp of when the workout was last updated.
             * @example 2021-09-14T12:00:00Z
             */
            updated_at?: string;
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export type operations = Record<string, never>;
