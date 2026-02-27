FROM debezium/connect:2.6

# Install the Confluent Avro Converter from the Confluent Maven repository so
# Debezium can serialise Kafka messages as Avro and register their schemas in
# Confluent Schema Registry.
ARG CONFLUENT_VERSION=7.6.0
RUN mkdir -p /kafka/connect/confluent-avro-converter && \
    curl -sL "https://packages.confluent.io/maven/io/confluent/kafka-connect-avro-converter/${CONFLUENT_VERSION}/kafka-connect-avro-converter-${CONFLUENT_VERSION}.jar" \
         -o "/kafka/connect/confluent-avro-converter/kafka-connect-avro-converter.jar" && \
    curl -sL "https://packages.confluent.io/maven/io/confluent/kafka-avro-serializer/${CONFLUENT_VERSION}/kafka-avro-serializer-${CONFLUENT_VERSION}.jar" \
         -o "/kafka/connect/confluent-avro-converter/kafka-avro-serializer.jar" && \
    curl -sL "https://packages.confluent.io/maven/io/confluent/kafka-schema-registry-client/${CONFLUENT_VERSION}/kafka-schema-registry-client-${CONFLUENT_VERSION}.jar" \
         -o "/kafka/connect/confluent-avro-converter/kafka-schema-registry-client.jar" && \
    curl -sL "https://packages.confluent.io/maven/io/confluent/kafka-schema-serializer/${CONFLUENT_VERSION}/kafka-schema-serializer-${CONFLUENT_VERSION}.jar" \
         -o "/kafka/connect/confluent-avro-converter/kafka-schema-serializer.jar"
