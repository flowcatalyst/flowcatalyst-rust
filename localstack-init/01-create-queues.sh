#!/bin/bash
# Create SQS queues for local development

echo "Creating SQS queues in LocalStack..."

# Create standard queues
awslocal sqs create-queue --queue-name fc-high-priority
awslocal sqs create-queue --queue-name fc-low-priority
awslocal sqs create-queue --queue-name fc-default

# Create FIFO queues (for message ordering)
awslocal sqs create-queue --queue-name fc-high-priority.fifo --attributes '{"FifoQueue":"true","ContentBasedDeduplication":"true"}'
awslocal sqs create-queue --queue-name fc-low-priority.fifo --attributes '{"FifoQueue":"true","ContentBasedDeduplication":"true"}'
awslocal sqs create-queue --queue-name fc-default.fifo --attributes '{"FifoQueue":"true","ContentBasedDeduplication":"true"}'

echo "SQS queues created successfully!"

# List queues to confirm
awslocal sqs list-queues
